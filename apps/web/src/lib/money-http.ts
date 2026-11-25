import { HttpError } from './http';

type MoneyEndpoint = { origin: string; parent: string; pk: string };

// Money returns RFC 9457 problems with a primary code and optional details.
// Only the matching operation, status and code can select purchase-specific copy.
function moneyError(path: string, status: number, problem: unknown) {
  const body = problem as { code?: unknown; details?: unknown } | null;
  if (path === 'portal_sessions:userCreate' && status === 412 && body?.code === 'INVALID_STATE' &&
      Array.isArray(body.details) && body.details.some(detail => detail?.reason === 'no_customer')) return 'no_customer';
  if (path.startsWith('checkout_sessions/') && path.endsWith(':claim')) {
    if (status === 409 && body?.code === 'RESOURCE_ALREADY_EXISTS') return 'already_claimed';
    if (status === 404 && body?.code === 'RESOURCE_NOT_FOUND') return 'not_found';
  }
  return status === 403 ? 'billing_refused' : status === 429 ? 'rate_limited' : 'billing_unavailable';
}

export async function moneyCall(money: MoneyEndpoint, path: string, token: string, data?: object, userAgent?: string, idempotency?: string, privileged = false) {
  const {status, result} = await requestMoney(money, path, token, data, userAgent, idempotency, privileged);
  if (status < 200 || status >= 300) {
    // Preserve the upstream refusal status without exposing provider diagnostics.
    throw new HttpError(status >= 500 ? 502 : status, moneyError(path, status, result));
  }
  if (!result || typeof result !== 'object') throw new HttpError(502, 'billing_unavailable');
  return result;
}

async function requestMoney(money: MoneyEndpoint, path: string, token: string, data?: object, userAgent?: string, idempotency?: string, privileged = false, parent = money.parent) {
  const response = await fetch(`${money.origin}/v1/${privileged ? '' : 'money/'}${parent}/${path}`, {
    method:data === undefined ? 'GET' : 'POST',
    headers:{
      Authorization: `Bearer ${token}`,
      ...(privileged ? {} : {'X-Sylphx-Key': money.pk}),
      ...(userAgent ? {'User-Agent':userAgent} : {}),
      ...(data === undefined ? {} : {'Content-Type':'application/json'}),
      ...(idempotency ? {'Idempotency-Key':idempotency} : {}),
    },
    body:data === undefined ? undefined : JSON.stringify(data), cache:'no-store', redirect:'error', signal:AbortSignal.timeout(12_000),
  });
  return {status:response.status, result:await response.json().catch(() => null)};
}

export async function readSettledCheckout(money: MoneyEndpoint, sessionId: string, billingKey: string) {
  // Redeem needs only the public receipt. Use the environment-scoped
  // billing:read key for the checkout resource, not the public redeem door.
  const receipt = await requestMoney(money, 'checkout_sessions:redeem', money.pk, {checkout_session:sessionId}, undefined, undefined, true, 'money');
  const name = receipt.result?.checkout_session;
  const prefix = money.parent + '/';
  if (receipt.status !== 200 || receipt.result?.paid !== true || typeof name !== 'string' || !name.startsWith(prefix)) return;
  const path = name.slice(prefix.length);
  if (!/^checkout_sessions\/chk[-_][A-Za-z0-9_-]{1,124}$/.test(path) || path !== path.trim()) return;
  const checkout = await requestMoney(money, path, billingKey, undefined, undefined, undefined, true);
  if (checkout.status === 200 && checkout.result?.name === name) return checkout.result;
}
