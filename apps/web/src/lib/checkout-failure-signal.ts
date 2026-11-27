import { createHash } from 'node:crypto';
import type { EntrySurface } from './entry-surface';
import { HttpError } from './http';
import { signalConfig } from './signal-config';
import { checkoutAnalyticsAllowed } from './signal-server';

// Money answered the create call and refused it, so no checkout exists for this intent.
// Timeouts, network errors, unreadable success bodies, 408, 409 and 5xx leave the outcome
// unknown: Money may still hold a session for the idempotency key. They are never reported.
const refusals = new Map([
  [400, 'rejected'], [404, 'rejected'], [412, 'rejected'], [422, 'rejected'],
  [401, 'refused'], [403, 'refused'], [429, 'rate_limited'],
] as const);

export function checkoutRefusal(moneyError: unknown) {
  return moneyError instanceof HttpError ? refusals.get(moneyError.status as never) : undefined;
}

// Optional analytics. Call it only with the error thrown by the Money create call itself,
// after input, sign-in and terms checks have passed. It never throws and never delays the refusal.
export async function reportCheckoutFailed(request: Request, browserGrant: unknown, checkoutIntent: string,
  entrySurface: EntrySurface, moneyError: unknown, send = fetch) {
  try {
    const status = checkoutRefusal(moneyError);
    const config = signalConfig();
    if (!status || !config || !/^[a-f0-9]{64}$/.test(checkoutIntent) || !checkoutAnalyticsAllowed(request, browserGrant)) return;
    // A retry reuses the browser intent and Money's idempotency key, so Signal keeps one record.
    const event_id = createHash('sha256').update('anymd:checkout_failed:' + checkoutIntent).digest('hex');
    const response = await send(config.baseUrl + '/v1/analytics/events:batch', {
      method:'POST', redirect:'error', cache:'no-store', signal:AbortSignal.timeout(10_000),
      headers:{'X-Sylphx-Key':config.apiKey, 'Content-Type':'application/json'},
      body:JSON.stringify({consent:'granted', events:[{
        event_id, event_name:'checkout_failed', distinct_id:event_id, properties:{status, entry_surface:entrySurface},
      }]}),
    });
    await response.body?.cancel();
  } catch { /* Signal transport failures cannot change the checkout refusal. */ }
}
