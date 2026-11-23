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
  const response = await fetch(`${money.origin}/v1/${privileged ? '' : 'money/'}${money.parent}/${path}`, {
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
  const result = await response.json().catch(() => null);
  if (!response.ok) {
    // Preserve the upstream refusal status without exposing provider diagnostics.
    throw new HttpError(response.status >= 500 ? 502 : response.status, moneyError(path, response.status, result));
  }
  if (!result || typeof result !== 'object') throw new HttpError(502, 'billing_unavailable');
  return result;
}
