import type { APIEvent } from '@solidjs/start/server';
import { getCookie, setCookie } from '@solidjs/start/http';
import { acceptTicket, authCall, authConfig, clientCapabilities, cookieOptions, oauthFinish, oauthStart, requireAuth, session, signOut } from '@/lib/auth';
import { noCapabilities } from '@/lib/auth-capabilities';
import { completeCode, resendDelaySeconds } from '@/lib/sign-in-flow';
import { appOrigin, failure, field, HttpError, json, redirect, sameOriginJson, safeNext } from '@/lib/http';

const challengePath = '/api/auth';
const emailPattern = /^[^\s@]{1,128}@[^\s@]{1,124}\.[^\s@]{1,32}$/;

export async function GET({request, params}: APIEvent) {
  const {action} = params;
  if (action === 'session') {
    try {
      const config = authConfig();
      const active = config && await session(request, config);
      const capabilities = new URL(request.url).searchParams.has('providers')
        ? config ? await clientCapabilities(request, config).catch(() => noCapabilities) : noCapabilities
        : { providers: [] };
      return json({authConfigured: Boolean(config), ...capabilities, user: active ? {name: active.user.name, email: active.user.email} : null});
    } catch (err) { return failure(err); }
  }
  try {
    if (action === 'google' || action === 'github') return redirect(await oauthStart(request, requireAuth(), action));
    if (action === 'callback') return redirect(appOrigin(request) + await oauthFinish(request, requireAuth()));
    return json({code:'not_found'}, 404);
  } catch (err) {
    if (['google','github','callback'].includes(action)) {
      const code = err instanceof HttpError ? err.code : 'auth_unavailable';
      return redirect(`/sign-in?error=${code}`);
    }
    return failure(err);
  }
}

export async function POST({request, params}: APIEvent) {
  try {
    const data = await sameOriginJson(request);
    const {action} = params;
    if (action === 'sign-out') { await signOut(request); return json({ok:true}); }
    const config = requireAuth();
    const next = safeNext(data.next);
    if (action === 'sign-in') {
      // Auth's Continue owns user creation after mailbox verification. Existing
      // and first-time users take the same path; this app never finds by email.
      if (data.code === undefined) {
        if (getCookie('anymd_email_wait')) throw new HttpError(429, 'rate_limited', String(resendDelaySeconds));
        const start = await authCall(config, request, '/v1/client/continue', config.pk, {email:field(data, 'email', emailPattern), method:'email_otp', session_mode:'server'});
        if (start?.step !== 'email_otp' || typeof start.flow !== 'string' || !/^[\w-]{43}$/.test(start.flow)) throw new HttpError(502, 'auth_unavailable');
        setCookie('anymd_email_flow', start.flow, cookieOptions(request, challengePath, 600));
        setCookie('anymd_email_wait', '1', cookieOptions(request, challengePath, resendDelaySeconds));
        return json({step:'email_otp'});
      }
      const flow = getCookie('anymd_email_flow');
      if (!flow) throw new HttpError(401, 'auth_refused');
      const proof = await authCall(config, request, '/v1/client/continue/verify', config.pk, {flow, code:field(data, 'code', completeCode)});
      await acceptTicket(request, config, proof?.ticket);
      setCookie('anymd_email_flow', '', cookieOptions(request, challengePath, 0));
      return json({next});
    }
    if (action === 'passkey-begin' || action === 'passkey-register-begin') {
      const origin = appOrigin(request);
      const rp_id = new URL(origin).hostname;
      const registering = action === 'passkey-register-begin';
      const active = registering ? await session(request, config) : null;
      if (registering && !active) throw new HttpError(401, 'auth_refused');
      const path = registering ? '/v1/client/me/passkeys/begin' : '/v1/client/sign-in/passkey/begin';
      const body = await authCall(config, request, path, active?.token ?? config.pk, {rp_id, origin}, registering);
      if (typeof body?.challenge_id !== 'string' || !/^[\w-]{1,128}$/.test(body.challenge_id) || body.rp_id !== rp_id) throw new HttpError(502,'auth_unavailable');
      setCookie(registering ? 'anymd_passkey_enroll' : 'anymd_passkey_flow', body.challenge_id, cookieOptions(request, challengePath, registering ? 120 : 600));
      return json(body);
    }
    if (action === 'passkey-verify' || action === 'passkey-register-verify') {
      const registering = action === 'passkey-register-verify';
      const key = registering ? 'anymd_passkey_enroll' : 'anymd_passkey_flow';
      const challenge_id = getCookie(key);
      if (!challenge_id || challenge_id !== data.challenge_id) throw new HttpError(401, 'auth_refused');
      const common = {challenge_id, credential_id:field(data, 'credential_id', /^[\w-]{1,2048}$/), client_data_json:field(data, 'client_data_json', /^[\w-]{1,8192}$/)};
      if (registering) {
        const active = await session(request, config);
        if (!active) throw new HttpError(401, 'auth_refused');
        const transports = Array.isArray(data.transports) ? data.transports.filter((v) => ['ble','hybrid','internal','nfc','usb','smart-card'].includes(v)).slice(0,6) : [];
        await authCall(config, request, '/v1/client/me/passkeys/verify', active.token, {...common, attestation_object:field(data, 'attestation_object', /^[\w-]{1,16000}$/), transports}, true);
      } else {
        const proof = await authCall(config, request, '/v1/client/sign-in/passkey/verify', config.pk, {...common, authenticator_data:field(data,'authenticator_data', /^[\w-]{1,4096}$/), signature:field(data,'signature', /^[\w-]{1,4096}$/), user_handle:field(data,'user_handle', /^[\w-]{0,2048}$/), session_mode:'server'});
        await acceptTicket(request, config, proof?.ticket);
      }
      setCookie(key, '', cookieOptions(request, challengePath, 0));
      return json({next});
    }
    return json({code:'not_found'},404);
  } catch (err) { return failure(err); }
}
