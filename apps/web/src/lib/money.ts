import 'server-only';
import { moneyAccessToken, requireAuth, session } from './auth';
import { appOrigin, canonicalOrigin, HttpError } from './http';
import { productionMoney } from './money-production.generated';
import { product, tokenProduct } from './catalog';

type Money = typeof productionMoney;

export function checkoutKey(request: Request, money: Money) {
  const live = money.parent === productionMoney.parent && appOrigin(request) === canonicalOrigin;
  const key = (live ? process.env.SYLPHX_MONEY_API_KEY : process.env.ANYMD_MONEY_TEST_API_KEY)?.trim();
  if (!key?.startsWith(live ? 'sylphx_sk_live_' : 'sylphx_sk_test_')) throw new HttpError(503, 'checkout_unconfigured');
  return key;
}
export function moneyConfig(request: Request): Money | null {
  // No production fallback in a preview. Test keys are issued by Money for a
  // separate environment; no browser option can choose a tenant or a key.
  const origin = appOrigin(request);
  if (origin === canonicalOrigin && request.headers.get('host') === new URL(canonicalOrigin).host && process.env.ANYMD_MONEY_MODE === 'live') return productionMoney;
  const pk = process.env.ANYMD_MONEY_TEST_KEY?.trim();
  const parent = process.env.ANYMD_MONEY_TEST_PARENT?.trim();
  if (!pk?.startsWith('sylphx_pk_test_') || !parent || parent === productionMoney.parent || !/^orgs\/org_[a-z0-9]+\/projects\/prj_[a-z0-9]+\/envs\/env_[a-z0-9]+$/.test(parent)) return null;
  const endpoint = new URL(process.env.ANYMD_MONEY_TEST_URL || productionMoney.origin);
  if (endpoint.protocol !== 'https:' && (endpoint.protocol !== 'http:' || !['localhost','127.0.0.1'].includes(endpoint.hostname))) return null;
  return { origin: endpoint.origin, parent, pk };
}

export async function moneyContext(request: Request) {
  const config = requireAuth();
  const active = await session(request, config);
  if (!active) throw new HttpError(401, 'unauthorized');
  const money = moneyConfig(request);
  if (!money) throw new HttpError(503, 'billing_unconfigured');
  return { config, ...active, money, accessToken: () => moneyAccessToken(request, config, active.token) };
}

const serviceErrors = new Map([
  ['no_customer', 'no_customer'],
  ['resource_already_exists', 'already_claimed'],
  ['not_found', 'not_found'],
  ['resource_not_found', 'not_found'],
]);

export async function moneyCall(money: Money, path: string, token: string, data?: object, userAgent?: string, idempotency?: string, privileged = false) {
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
    const details = Array.isArray(result?.error?.details) ? result.error.details : [];
    const codes = [result?.code, result?.reason, result?.error?.code, result?.error?.status, ...details.map((d: { reason?: unknown }) => d?.reason)];
    const known = codes.find((code) => typeof code === 'string' && serviceErrors.has(code.toLowerCase()));
    const code = known ? serviceErrors.get(known.toLowerCase())! : response.status === 403 ? 'billing_refused' : response.status === 429 ? 'rate_limited' : 'billing_unavailable';
    // Do not send processor responses, PII or diagnostic content to the page.
    throw new HttpError(response.status >= 500 ? 502 : response.status, code);
  }
  if (!result || typeof result !== 'object') throw new HttpError(502, 'billing_unavailable');
  return result;
}

export function processorUrl(value: unknown, door: 'checkout' | 'billing-portal') {
  if (typeof value !== 'string') throw new HttpError(502, 'billing_unavailable');
  const url = new URL(value);
  const host = door === 'checkout' ? 'checkout.stripe.com' : 'billing.stripe.com';
  if (url.origin !== `https://${host}` || url.username || url.password) throw new HttpError(502, 'billing_unavailable');
  return url.toString();
}

export async function entitlement(ctx: Awaited<ReturnType<typeof moneyContext>>, jwt: string) {
  const body = await moneyCall(ctx.money, 'entitlement_grants:getMine?feature=' + product.key, jwt);
  if (body.subject_id !== ctx.user.id || typeof body.entitled !== 'boolean') throw new HttpError(502, 'billing_unavailable');
  return body.entitled as boolean;
}

export async function licenceTokens(ctx: Awaited<ReturnType<typeof moneyContext>>, jwt: string) {
  const query = new URLSearchParams({filter:`end_user = "${ctx.user.id}" AND token_product = "${tokenProduct}"`});
  const body = await moneyCall(ctx.money, 'licence_tokens:listMine?' + query, jwt);
  return readTokens(body);
}

export function readTokens(body: {licence_tokens?: unknown}) {
  if (!Array.isArray(body.licence_tokens)) throw new HttpError(502, 'billing_unavailable');
  return body.licence_tokens.flatMap((row) => typeof row?.token === 'string' && /^[\w-]{1,4096}\.[\w-]{1,1024}$/.test(row.token) ? [row.token as string] : []);
}
