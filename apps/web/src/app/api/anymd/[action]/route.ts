import { currencyOptions, price, product, tokenProduct } from '@/lib/catalog';
import { consentText, consentVersion } from '@/lib/money-public.generated';
import { appOrigin, failure, field, HttpError, json, sameOriginJson } from '@/lib/http';
import { checkoutKey, entitlement, moneyCall, moneyConfig, moneyContext, processorUrl, readTokens } from '@/lib/money';

type Params = { params: Promise<{action:string}> };

export async function GET(request: Request, {params}: Params) {
  if ((await params).action !== 'account') return json({code:'not_found'},404);
  try {
    const ctx = await moneyContext(request);
    const jwt = await ctx.accessToken();
    const active = await entitlement(ctx, jwt);
    // getMine returns the effective feature, not grant rows. Never imply access
    // from an order, a receipt query parameter or a locally inferred plan.
    return json({plan:active ? 'Anymd Pro' : 'Free OSS', entitlementActive:active, features:active ? [product.key] : []});
  } catch (err) { return failure(err); }
}

export async function POST(request: Request, {params}: Params) {
  try {
    const data = await sameOriginJson(request);
    const {action} = await params;
    if (action === 'recover-legacy') {
      const cfg = moneyConfig(request);
      if (!cfg) throw new HttpError(503, 'billing_unconfigured');
      const proof = field(data, 'recoveryToken', /^[\w-]{1,4096}\.[\w-]{1,1024}$/);
      const body = await moneyCall(cfg, 'licence_tokens:recover', cfg.pk, {recovery_token:proof});
      return json({tokens:readTokens(body)});
    }
    const ctx = await moneyContext(request);
    const origin = appOrigin(request);
    const subject = {end_user:ctx.user.id};
    if (action === 'claim') {
      const id = field(data, 'checkoutSessionId', /^(?:chk|cs)_[a-zA-Z0-9_-]{1,124}$/);
      // Money validates possession or the verified payer email, never an
      // address submitted by this browser. Preserve the opaque Auth session
      // for Money's verified-email door rather than an email-less JWT.
      const claim_secret = data.claimSecret === undefined ? undefined : field(data,'claimSecret', /^[\w-]{8,512}$/);
      const result = await moneyCall(ctx.money, `checkout_sessions/${id}:claim`, ctx.token, claim_secret ? {claim_secret} : {}, request.headers.get('user-agent') ?? undefined);
      return json({claimed:result.claimed === true && result.claimed_by === ctx.user.id});
    }
    if (action === 'checkout') {
      if (!currencyOptions.some(({code}) => code === data.currency) || data.consent !== true) throw new HttpError(400, 'invalid_request');
      const key = field(data,'checkoutIntent', /^[a-f0-9-]{36}$/);
      const keyForMoney = checkoutKey(request, ctx.money);
      // The BFF has already authenticated this exact subject. The privileged
      // server door alone may carry merchant-owned hosted waiver policy.
      const result = await moneyCall(ctx.money, 'checkout_sessions', keyForMoney, {
        subject, line_items:[{price:price.key, quantity:1}], currency_code:data.currency,
        success_url:`${origin}/pro/thanks?session_id={CHECKOUT_SESSION_ID}`,
        cancel_url:`${origin}/pricing?checkout=cancelled`,
        consent_collection:{terms_required:true, terms_text:consentText, terms_version:consentVersion},
      }, undefined, `anymd-${ctx.user.id}-${key}`, true);
      return json({url:processorUrl(result.url,'checkout')});
    }
    if (action === 'billing-portal') {
      const jwt = await ctx.accessToken();
      const result = await moneyCall(ctx.money, 'portal_sessions:userCreate', jwt, {subject, return_url:`${origin}/billing`});
      return json({url:processorUrl(result.url,'billing-portal')});
    }
    return json({code:'not_found'},404);
  } catch (err) { return failure(err); }
}
