import { createHash } from 'node:crypto';
import { consentKey, explicitAnalytics } from './consent';
import { checkoutParameters } from './funnel';
import type { EntrySurface } from './entry-surface';
import { signalConfig } from './signal-config';

function checkoutAnalyticsAllowed(request: Request, browserGrant: unknown) {
  // The browser also checks navigator GPC. Require its grant AND a single
  // stored choice in the incoming cookie; never infer consent from checkout terms.
  if (browserGrant !== true || request.headers.has('Sec-GPC')) return false;
  const choices = (request.headers.get('Cookie') ?? '').split(';').map(cookie => cookie.trim())
    .filter(cookie => cookie.startsWith(consentKey + '='));
  return choices.length === 1 && explicitAnalytics(choices[0].slice(consentKey.length + 1), request.headers);
}

export async function reportCheckoutStarted(request: Request, browserGrant: unknown, checkoutSessionId: string,
  currency: string, entrySurface: EntrySurface, send = fetch) {
  try {
    if (!/^chk-[a-f0-9]{24}$/.test(checkoutSessionId) || !checkoutAnalyticsAllowed(request, browserGrant)) return;
    const config = signalConfig();
    const parameters = checkoutParameters(currency, entrySurface);
    if (!config || !parameters) return;
    const {value, plan_sku} = parameters;
    // Money can return the same session for a retried checkout intent. Use its
    // stable ID for dedup without sending the raw session or end-user identity.
    const event_id = createHash('sha256').update('anymd:checkout_started:' + checkoutSessionId).digest('hex');
    const response = await send(config.baseUrl + '/v1/analytics/events:batch', {
      method:'POST', redirect:'error', cache:'no-store', signal:AbortSignal.timeout(10_000),
      headers:{'X-Sylphx-Key':config.apiKey, 'Content-Type':'application/json'},
      body:JSON.stringify({consent:'granted', events:[{
        event_id, event_name:'checkout_started', distinct_id:event_id, numeric_value:value,
        properties:{currency, plan_sku, entry_surface:entrySurface},
      }]}),
    });
    await response.body?.cancel();
  } catch { /* Signal transport failures cannot block checkout. */ }
}
