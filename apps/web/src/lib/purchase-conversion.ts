import { createHash, createHmac, timingSafeEqual } from 'node:crypto';
import { resolveEffectiveConsent } from '@sylphx/sdk/signal';
import { fullAdConsent, googleAdsConfig, liveCheckoutSession, type PurchaseConversion } from './google-ads';
import { appOrigin, failure, HttpError, json, sameOriginJson } from './http';
import { checkoutKey, moneyContext } from './money';
import { readSettledCheckout } from './money-http';

const path = '/api/anymd/purchase-conversion';
const cookieName = '__Secure-anymd_ads_claims';
const limit = 32;
const noClaim = () => new Response(null, {status:204, headers:{'Cache-Control':'private, no-store'}});

function sign(secret: string, scope: string, ids: string) {
  return createHmac('sha256', secret).update(JSON.stringify(['anymd-ads-claims/v1', scope, ids])).digest('hex');
}

function claimedIds(request: Request, secret: string, scope: string) {
  const cookie = request.headers.get('cookie')?.split(';').map(part => part.trim()).find(part => part.startsWith(cookieName + '='));
  const [ids, mac, ...extra] = cookie?.slice(cookieName.length + 1).split('~') ?? [];
  if (!ids || !mac || extra.length || !/^[a-f0-9]{64}$/.test(mac) || mac.length !== 64) return [];
  const list = ids.split('.');
  if (list.length > limit || !list.every(id => id.length === 32 && /^[a-f0-9]+$/.test(id))) return [];
  return timingSafeEqual(Buffer.from(mac, 'hex'), Buffer.from(sign(secret, scope, ids), 'hex')) ? list : [];
}

export async function purchaseConversion(request: Request) {
  try {
    const data = await sameOriginJson(request);
    if (data.ads_consent !== true || !fullAdConsent(resolveEffectiveConsent({storedChoice:'granted'}, request.headers))) {
      throw new HttpError(403, 'ads_consent_required');
    }
    const ctx = await moneyContext(request);
    const sid = data.session_id;
    if (!liveCheckoutSession(sid) || !googleAdsConfig(process.env.ANYMD_GOOGLE_ADS_ID, process.env.ANYMD_GOOGLE_ADS_PURCHASE_LABEL)) return noClaim();
    const checkout = await readSettledCheckout(ctx.money, sid, checkoutKey(request, ctx.money));
    if (!checkout || checkout.subject?.end_user !== ctx.user.id || checkout.processor_session_id !== sid ||
        !(checkout.livemode === undefined || checkout.livemode === true) || checkout.state !== 'complete' ||
        checkout.payment_status !== 'paid' || checkout.denied_reason) return noClaim();
    const {amount_total:rawAmount, currency_code:currency} = checkout;
    const amount = typeof rawAmount === 'string' && /^[1-9][0-9]{0,15}$/.test(rawAmount) && rawAmount === rawAmount.trim() ? Number(rawAmount) : rawAmount;
    if (typeof currency !== 'string' || currency.length !== 3 || !/^[A-Z]{3}$/.test(currency) ||
        typeof amount !== 'number' || !Number.isSafeInteger(amount) || amount <= 0) return noClaim();
    const decimals = new Intl.NumberFormat('en', {style:'currency', currency}).resolvedOptions().maximumFractionDigits ?? 2;
    const transactionId = createHash('sha256').update('anymd-ads-purchase:' + checkout.name).digest('hex').slice(0, 32);
    const scope = JSON.stringify([appOrigin(request), ctx.money.parent, ctx.user.id]);
    const ids = claimedIds(request, ctx.config.sk, scope);
    if (ids.includes(transactionId)) return noClaim();
    // The bounded cookie suppresses reloads. Concurrent claims and other
    // devices still share Google's canonical deduplication ID.
    const list = [transactionId, ...ids].slice(0, limit).join('.');
    const signed = list + '~' + sign(ctx.config.sk, scope, list);
    const result: PurchaseConversion = {transactionId, value:amount / 10 ** decimals, currency};
    return json(result, 200, {'Set-Cookie':cookieName + '=' + signed + '; Path=' + path + '; Max-Age=7776000; HttpOnly; Secure; SameSite=Lax'});
  } catch (error) { return failure(error); }
}
