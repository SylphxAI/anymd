import { requireAuth, session, verifySessionUser } from '@/lib/auth';
import { challengePattern, redeemGrant, sealGrant, statePattern } from '@/lib/cli-grant';
import { appOrigin, failure, field, HttpError, json, redirect } from '@/lib/http';
import { entitlement, licenceTokens, moneyContextForSession } from '@/lib/money';

export async function POST(request: Request) {
  try {
    const config = requireAuth();
    const contentType = request.headers.get('content-type')?.split(';', 1)[0].trim().toLowerCase();
    if (contentType === 'application/x-www-form-urlencoded') {
      if (request.headers.get('origin') !== appOrigin(request) || request.headers.get('sec-fetch-site') === 'cross-site') throw new HttpError(403, 'invalid_origin');
      const data = new URLSearchParams(await shortBody(request, 512));
      const port = data.get('port') ?? '';
      const state = data.get('state') ?? '';
      const challenge = data.get('challenge') ?? '';
      if (!/^\d{1,5}$/.test(port) || Number(port) < 1024 || Number(port) > 65535 || !statePattern.test(state) || !challengePattern.test(challenge)) throw new HttpError(400, 'invalid_request');
      const active = await session(request, config);
      if (!active) throw new HttpError(401, 'unauthorized');
      const grant = sealGrant(config.sk, config.organization + appOrigin(request), challenge, active.user.id, active.token);
      return redirect('http://127.0.0.1:' + Number(port) + '/callback?' + new URLSearchParams({state, grant}));
    }
    // Native backchannel only. Browser pages may not redeem grants by form or fetch.
    if (contentType !== 'application/json' || request.headers.has('origin')) throw new HttpError(403, 'invalid_request');
    let data: Record<string, unknown>;
    try { data = JSON.parse(await shortBody(request, 9000)); } catch { throw new HttpError(400, 'invalid_request'); }
    if (!data || typeof data !== 'object' || Array.isArray(data)) throw new HttpError(400, 'invalid_request');
    const grant = redeemGrant(config.sk, config.organization + appOrigin(request), field(data, 'grant', /^[\w-]{40,8192}$/), field(data, 'verifier', challengePattern));
    if (!grant) throw new HttpError(401, 'grant_refused');
    const user = await verifySessionUser(config, request, grant.token);
    if (!user || user.id !== grant.subject) throw new HttpError(401, 'unauthorized');
    const ctx = moneyContextForSession(request, config, {user, token: grant.token});
    if (!ctx) return json({plan:'unavailable', tokens:[]});
    const jwt = await ctx.accessToken();
    if (!await entitlement(ctx, jwt)) return json({plan:'free', tokens:[]});
    return json({plan:'pro', tokens:await licenceTokens(ctx, jwt)});
  } catch (error) { return failure(error); }
}

async function shortBody(request: Request, limit: number) {
  const reader = request.body?.getReader();
  let size = 0;
  const decoder = new TextDecoder();
  let text = '';
  while (reader) {
    const {value, done} = await reader.read();
    if (done) break;
    size += value.length;
    if (size > limit) { await reader.cancel(); throw new HttpError(413, 'invalid_request'); }
    text += decoder.decode(value, {stream:true});
  }
  return text + decoder.decode();
}
