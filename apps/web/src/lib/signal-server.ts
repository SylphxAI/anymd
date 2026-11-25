import { createHash } from 'node:crypto';
import { isCurrency, price } from './catalog';
import { consentKey, explicitAnalytics } from './consent';
import { checkoutParameters } from './funnel';
import { knownEntrySurface, type EntrySurface } from './entry-surface';
import { signalConfig } from './signal-config';

export function checkoutAnalyticsAllowed(request: Request, browserGrant: unknown) {
  // The browser also checks navigator GPC. Require its grant AND a single
  // stored choice in the incoming cookie; never infer consent from checkout terms.
  if (browserGrant !== true || request.headers.has('Sec-GPC')) return false;
  const choices = (request.headers.get('Cookie') ?? '').split(';').map(cookie => cookie.trim())
    .filter(cookie => cookie.startsWith(consentKey + '='));
  return choices.length === 1 && explicitAnalytics(choices[0].slice(consentKey.length + 1), request.headers);
}

type SignalConfig = NonNullable<ReturnType<typeof signalConfig>>;

async function sendCheckoutEvent(config: SignalConfig, event_name: 'checkout_started' | 'purchase_completed',
  checkoutName: string, value: number, currency: string, plan_sku: string, entry_surface: EntrySurface | undefined,
  signal: AbortSignal, send: typeof fetch) {
  // The same Money resource can be seen on retries and on other devices.
  // Signal deduplicates by event ID without receiving the raw resource or buyer identity.
  const event_id = createHash('sha256').update('anymd:' + event_name + ':' + checkoutName).digest('hex');
  const response = await send(config.baseUrl + '/v1/analytics/events:batch', {
    method:'POST', redirect:'error', cache:'no-store', signal,
    headers:{'X-Sylphx-Key':config.apiKey, 'Content-Type':'application/json'},
    body:JSON.stringify({consent:'granted', events:[{
      event_id, event_name, distinct_id:event_id, numeric_value:value,
      properties:{currency, plan_sku, entry_surface},
    }]}),
  });
  await response.body?.cancel();
}

export async function reportCheckoutStarted(request: Request, browserGrant: unknown, checkoutSessionId: string,
  currency: string, entrySurface: EntrySurface, send = fetch) {
  try {
    if (!/^chk-[a-f0-9]{24}$/.test(checkoutSessionId) || !checkoutAnalyticsAllowed(request, browserGrant)) return;
    const config = signalConfig();
    const parameters = checkoutParameters(currency, entrySurface);
    if (!config || !parameters) return;
    await sendCheckoutEvent(config, 'checkout_started', checkoutSessionId, parameters.value, currency, parameters.plan_sku,
      entrySurface, AbortSignal.timeout(10_000), send);
  } catch { /* Signal transport failures cannot block checkout. */ }
}

export async function signalPurchaseCompleted(request: Request, config: SignalConfig,
  checkout: {name: string; livemode?: unknown; line_items?: unknown; metadata?: {entry_surface?: unknown}},
  value: number, currency: string) {
  const items = checkout.line_items;
  if (request.signal.aborted || checkout.livemode !== true || !isCurrency(currency) || !Array.isArray(items) ||
      items.length !== 1 || items[0]?.price !== price.key) return;
  try {
    await sendCheckoutEvent(config, 'purchase_completed', checkout.name, value, currency, price.key,
      knownEntrySurface(checkout.metadata?.entry_surface), AbortSignal.any([request.signal, AbortSignal.timeout(750)]), fetch);
  } catch { /* Signal transport failures cannot block the return or the Ads claim. */ }
}
