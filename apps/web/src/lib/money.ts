import { type Config, moneyAccessToken, requireAuth, session, type User } from './auth';
import { appOrigin, canonicalOrigin, HttpError } from './http';
import { productionMoney } from './money-production.generated';
import { product, tokenProduct } from './catalog';
import { moneyCall } from './money-http';
import { testMoneyOrigin, testMoneyParent } from './money-target';

type Money = typeof productionMoney;

export function checkoutKey(request: Request, money: Money) {
  const live = money.parent === productionMoney.parent && liveMoneyHost(request);
  const key = (live ? process.env.SYLPHX_MONEY_API_KEY : process.env.ANYMD_MONEY_TEST_API_KEY)?.trim();
  if (!key?.startsWith(live ? 'sylphx_sk_live_' : 'sylphx_sk_test_')) throw new HttpError(503, 'checkout_unconfigured');
  return key;
}
function liveMoneyHost(request: Request) {
  const origin = appOrigin(request);
  const host = request.headers.get('host');
  const assigned = process.env.ANYMD_APP_ORIGIN?.trim();
  const staging = assigned === origin && process.env.SYLPHX_MONEY_API_KEY?.trim().startsWith('sylphx_sk_live_') && /^https:\/\/[a-z0-9-]+(?:\.[a-z0-9-]+)*\.sylphx\.app$/i.test(origin);
  return (origin === canonicalOrigin || staging) && host === new URL(origin).host && process.env.ANYMD_MONEY_MODE === 'live';
}

export function moneyConfig(request: Request): Money | null {
  // No production fallback in a preview. Test keys are issued by Money for a
  // separate environment; no browser option can choose a tenant or a key.
  if (liveMoneyHost(request)) return productionMoney;
  const pk = process.env.ANYMD_MONEY_TEST_KEY?.trim();
  const parent = testMoneyParent();
  if (!pk?.startsWith('sylphx_pk_test_') || !parent || parent === productionMoney.parent) return null;
  const origin = testMoneyOrigin();
  return origin ? { origin, parent, pk } : null;
}

export async function moneyContext(request: Request) {
  const config = requireAuth();
  const active = await session(request, config);
  if (!active) throw new HttpError(401, 'unauthorized');
  const ctx = moneyContextForSession(request, config, active);
  if (!ctx) throw new HttpError(503, 'billing_unconfigured');
  return ctx;
}

export function moneyContextForSession(request: Request, config: Config, active: {user: User; token: string}) {
  const money = moneyConfig(request);
  return money ? { config, ...active, money, accessToken: () => moneyAccessToken(request, config, active.token) } : null;
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
