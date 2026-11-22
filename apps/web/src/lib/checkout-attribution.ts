import { canonicalOrigin } from './app-origin';
import { clickCookie, clickIds, measurementId } from './attribution';
import { consentFor, consentKey, parseChoice } from './consent';
import { checkoutIdPattern } from './purchase';

const bindingTimeout = 750;
const gaSessionCookie = '_ga_' + measurementId.slice(2);

function cookieValues(header: string, name: string) {
  const prefix = name + '=';
  return header.split(';').map(part => part.trimStart()).filter(part => part.startsWith(prefix)).map(part => part.slice(prefix.length));
}

function measurementCookie(header: string, name: string, maxLength = 512) {
  const values = cookieValues(header, name);
  if (values.length !== 1 || values[0].length > maxLength) return '';
  try { return decodeURIComponent(values[0]); } catch { return ''; }
}

function gaSessionId(value: string) {
  const gs1 = /^GS1\.\d{1,2}\.([1-9]\d{0,19})(?:\.\d{1,20})*$/.exec(value);
  if (gs1) return gs1[1];
  const gs2 = /^GS2\.\d{1,2}\.([a-z][A-Za-z0-9-]*(?:\$[a-z][A-Za-z0-9-]*)*)$/.exec(value);
  const fields = gs2?.[1].split('$').filter(field => field.startsWith('s'));
  return fields?.length === 1 ? /^s([1-9]\d{0,19})$/.exec(fields[0])?.[1] : undefined;
}

function readClickIds(header: string) {
  try {
    const values = JSON.parse(measurementCookie(header, clickCookie, 768));
    return clickIds(key => values?.[key]);
  } catch { return {}; }
}

function binding(headers: Headers, checkoutSessionId: string, effectiveChoice: unknown) {
  if (!checkoutIdPattern.test(checkoutSessionId) || headers.get('sec-gpc')?.split(',').some(value => value.trim() === '1')) return;
  const choice = parseChoice(effectiveChoice);
  const cookies = headers.get('cookie') ?? '';
  const saved = cookieValues(cookies, consentKey);
  // No grant inferred from a missing choice, tracking cookies or region. A
  // saved restriction can only narrow the browser's effective checkout choice.
  const preference = parseChoice(saved[0]);
  if (saved.length > 1 || (saved.length === 1 && (!preference || preference === 'reject'))) return;
  const consent = consentFor('opt-in', preference === 'analytics' && choice === 'all' ? 'analytics' : choice, false);
  if (consent.analytics_storage !== 'granted') return;
  const adsAllowed = consent.ad_storage === 'granted' && consent.ad_user_data === 'granted';
  return {
    checkout_session_id: checkoutSessionId,
    consent,
    ga_client_id: /^GA1\.\d{1,2}\.(\d{1,20}\.\d{1,20})$/.exec(measurementCookie(cookies, '_ga'))?.[1],
    ga_session_id: gaSessionId(measurementCookie(cookies, gaSessionCookie)),
    ...(adsAllowed && {
      ...readClickIds(cookies),
      fbc: /^fb\.[1-9]\d?\.\d{13}\.[A-Za-z0-9_-]{1,200}$/.exec(measurementCookie(cookies, '_fbc'))?.[0],
      fbp: /^fb\.[1-9]\d?\.\d{13}\.\d{1,20}$/.exec(measurementCookie(cookies, '_fbp'))?.[0],
    }),
    page_location: canonicalOrigin + '/pricing',
  };
}

// The checkout route starts this request without awaiting it. The server-owned
// origin and existing project key come from the same Money configuration.
export async function bindCheckoutAttribution(money: { origin: string; pk: string }, headers: Headers, checkoutSessionId: string, choice: unknown) {
  try {
    const body = binding(headers, checkoutSessionId, choice);
    if (!body) return;
    const result = await fetch(money.origin + '/v1/events/attribution_bindings', {
      method: 'POST',
      headers: {'Content-Type': 'application/json', 'X-Sylphx-Key': money.pk},
      body: JSON.stringify(body), cache: 'no-store', redirect: 'error', signal: AbortSignal.timeout(bindingTimeout),
    });
    void result.body?.cancel().catch(() => undefined);
    if (!result.ok) console.warn('Checkout attribution binding rejected', result.status);
  } catch {
    // Never log request data, upstream details or the exception itself.
    console.warn('Checkout attribution binding unavailable');
  }
}
