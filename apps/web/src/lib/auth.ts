import { createHmac, randomBytes, timingSafeEqual } from 'node:crypto';
import { getCookie, setCookie } from '@solidjs/start/http';
import { consentKey } from './consent';
import { signupCookie, signupSignal, type SignupMethod } from './signup';
import { parseCapabilities } from './auth-capabilities';
import { appOrigin, asText, HttpError, safeNext } from './http';

const sessionName = 'anymd_session';
export type Config = { origin: string; pk: string; sk: string; organization: string };
export type User = { id: string; name: string | null; email: string | null; emailVerified: boolean };
export function authConfig(): Config | null {
  const pk = process.env.SYLPHX_PUBLISHABLE_KEY?.trim();
  const sk = process.env.SYLPHX_AUTH_SECRET_KEY?.trim();
  const organization = instanceId(process.env.SYLPHX_AUTH_ORGANIZATION_ID ?? '');
  try {
    const url = new URL(process.env.SYLPHX_AUTH_URL?.trim() ?? '');
    const allowed = url.protocol === 'https:' || (url.protocol === 'http:' && ['127.0.0.1', 'localhost'].includes(url.hostname));
    return allowed && pk && sk && organization ? { origin: url.origin, pk, sk, organization } : null;
  } catch { return null; }
}

function instanceId(id: string) {
  const uuid = id.trim().replace(/^organization-/, '');
  if (/^[\da-f]{8}-[\da-f]{4}-[\da-f]{4}-[\da-f]{4}-[\da-f]{12}$/i.test(uuid)) return uuid.replaceAll('-', '').toLowerCase();
  const suffix = /^[a-z]{2,5}_([0-7][0-9a-hjkmnp-tv-z]{25})$/.exec(id.trim())?.[1];
  if (!suffix) return null;
  let value = 0n;
  for (const c of suffix) value = (value << 5n) | BigInt('0123456789abcdefghjkmnpqrstvwxyz'.indexOf(c));
  return value.toString(16).padStart(32, '0');
}

export function requireAuth() {
  const config = authConfig();
  if (!config) throw new HttpError(503, 'auth_unconfigured');
  return config;
}

export async function authCall(config: Config, request: Request, path: string, bearer: string, body?: object, callerBound = false) {
  const response = await fetch(config.origin + path, {
    method: body === undefined ? 'GET' : 'POST',
    headers: {
      'Authorization': `Bearer ${bearer}`,
      'User-Agent': request.headers.get('user-agent') || 'anymd-web',
      ...(callerBound ? { 'x-sylphx-caller-key': config.pk } : {}),
      ...(body === undefined ? {} : {'Content-Type': 'application/json'}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
    cache: 'no-store', redirect: 'error', signal: AbortSignal.timeout(10_000),
  });
  if (!response.ok) {
    const rawRetry = response.headers.get('retry-after');
    const retry = rawRetry && /^\d{1,6}$/.test(rawRetry) ? rawRetry : undefined;
    if (response.status === 429) throw new HttpError(429, 'rate_limited', retry);
    if (response.status === 401 || response.status === 403) throw new HttpError(response.status, 'auth_refused');
    throw new HttpError(response.status >= 500 ? 502 : 400, 'auth_unavailable');
  }
  return response.status === 204 ? {} : response.json();
}

export async function verifySessionUser(config: Config, request: Request, token: string): Promise<User | null> {
  let body;
  try { body = await authCall(config, request, '/v1/sessions/current', token, undefined, true); }
  catch (err) {
    if (err instanceof HttpError && [401, 403].includes(err.status)) return null;
    throw err;
  }
  const p = body?.session?.principal ?? body;
  const tenant = asText(p?.project_id ?? p?.projectId);
  const id = asText(p?.principal_id ?? p?.principalId ?? p?.id);
  if (!tenant || instanceId(tenant) !== config.organization || !id || !/^usr_[a-zA-Z0-9_-]{8,80}$/.test(id) || (p.state != null && p.state !== 'active') || p.guest === true) return null;
  return { id, name: asText(p.display_name ?? p.displayName), email: asText(p.primary_email ?? p.primaryEmail), emailVerified: (p.primary_email_verified ?? p.primaryEmailVerified) === true };
}

export async function session(request: Request, config: Config) {
  const token = sessionToken();
  if (!token) return null;
  const user = await verifySessionUser(config, request, token);
  return user ? { user, token } : null;
}
function sessionToken() { return getCookie(sessionName); }

export async function moneyAccessToken(request: Request, config: Config, token: string) {
  const body = await authCall(config, request, '/v1/client/token', token, {}, true);
  const jwt = asText(body?.access_token);
  if (!jwt || !/^[\w-]+\.[\w-]+\.[\w-]+$/.test(jwt)) throw new HttpError(502, 'auth_unavailable');
  return jwt;
}

export async function acceptTicket(request: Request, config: Config, ticket: unknown, method: SignupMethod | undefined, next: string) {
  if (typeof ticket !== 'string' || !/^[\w-]{20,512}$/.test(ticket)) throw new HttpError(401, 'auth_refused');
  const body = await authCall(config, request, '/v1/client/tickets:redeem', config.sk, {ticket});
  const token = asText(body?.session?.token);
  if (!token || !await verifySessionUser(config, request, token)) throw new HttpError(401, 'auth_refused');
  setCookie(sessionName, token, cookieOptions(request, '/', 30 * 86400));
  // Only a verified redemption can produce this optional redirect handoff.
  const signup = method && signupSignal(body?.is_new_user, getCookie(consentKey), request.headers.get('sec-gpc') === '1', method, next);
  if (signup) setCookie(signupCookie, signup.method + '.' + signup.entry_surface, {...cookieOptions(request, '/', 60), httpOnly:false});
}

export function cookieOptions(request: Request, path: string, maxAge: number) {
  return { httpOnly: true, secure: appOrigin(request).startsWith('https:'), sameSite: 'lax' as const, path, maxAge };
}

export async function signOut(request: Request) {
  const config = authConfig();
  const token = sessionToken();
  setCookie(sessionName, '', cookieOptions(request, '/', 0));
  if (config && token) await authCall(config, request, '/v1/client/sign-out', token, {}).catch(() => {});
}

const stateCookie = 'anymd_oauth_state';
const statePath = '/api/auth/callback';
function stateMac(config: Config, origin: string, payload: string) {
  return createHmac('sha256', config.sk).update(`anymd-state-v1\0${origin}\0${payload}`).digest('base64url');
}
export async function clientCapabilities(request: Request, config: Config) {
  const query = new URLSearchParams({publishable_key: config.pk});
  const body = await authCall(config, request, '/v1/client/config?' + query, config.pk);
  return parseCapabilities(body);
}

export async function oauthStart(request: Request, config: Config, provider: 'google' | 'github') {
  if (!(await clientCapabilities(request, config)).providers.includes(provider)) throw new HttpError(404, 'provider_unavailable');
  const nonce = randomBytes(24).toString('base64url');
  const next = safeNext(new URL(request.url).searchParams.get('next'));
  const origin = appOrigin(request);
  const payload = Buffer.from(JSON.stringify({nonce, next, provider, exp: Date.now() + 600_000})).toString('base64url');
  setCookie(stateCookie, `${payload}.${stateMac(config, origin, payload)}`, cookieOptions(request, statePath, 600));
  const callback = `${origin}${statePath}?state=${nonce}`;
  return `${config.origin}/v1/client/oauth/${provider}/start?${new URLSearchParams({publishable_key: config.pk, redirect_url: callback, session_mode:'server'})}`;
}
export async function oauthFinish(request: Request, config: Config) {
  const [payload, mac] = (getCookie(stateCookie) ?? '').split('.');
  setCookie(stateCookie, '', cookieOptions(request, statePath, 0));
  const expected = Buffer.from(stateMac(config, appOrigin(request), payload));
  if (!mac || mac.length !== expected.length || !timingSafeEqual(Buffer.from(mac), expected)) throw new HttpError(401, 'auth_refused');
  const state = JSON.parse(Buffer.from(payload, 'base64url').toString());
  const query = new URL(request.url).searchParams;
  if (state.exp <= Date.now() || state.nonce !== query.get('state')) throw new HttpError(401, 'auth_refused');
  const next = safeNext(state.next);
  const provider = state.provider === 'google' || state.provider === 'github' ? state.provider : undefined;
  await acceptTicket(request, config, query.get('sylphx_ticket'), provider, next);
  return next;
}
