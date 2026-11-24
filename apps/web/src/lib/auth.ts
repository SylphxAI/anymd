import { readAuthSessionCookie } from '@sylphx/auth';
import { asText, HttpError } from './http';

export const legacySessionCookies = ['anymd_session'] as const;
export type Config = { origin: string; pk: string; sk: string; organization: string };
export type User = { id: string; name: string | null; email: string | null; emailVerified: boolean };
export function authConfig(): Config | null {
  const pk = process.env.SYLPHX_PUBLISHABLE_KEY?.trim();
  const sk = process.env.SYLPHX_AUTH_SECRET_KEY?.trim();
  const organization = authTenantId(process.env.SYLPHX_AUTH_ORGANIZATION_ID ?? '');
  try {
    const url = new URL(process.env.SYLPHX_AUTH_URL?.trim() ?? '');
    const allowed = url.protocol === 'https:' && !url.username && !url.password;
    return allowed && pk?.startsWith('sylphx_pk_') && (sk?.startsWith('sylphx_sk_') || sk?.startsWith('identity_org_key_')) && organization ? { origin: url.origin, pk, sk, organization } : null;
  } catch { return null; }
}

export function authTenantId(id: string) {
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

async function authCall(config: Config, request: Request, path: string, bearer: string, body?: object) {
  const response = await fetch(config.origin + path, {
    method: body === undefined ? 'GET' : 'POST',
    headers: {
      'Authorization': `Bearer ${bearer}`,
      'User-Agent': request.headers.get('user-agent') || 'anymd-web',
      'x-sylphx-caller-key': config.pk,
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
  try { body = await authCall(config, request, '/v1/sessions/current', token); }
  catch (err) {
    if (err instanceof HttpError && [401, 403].includes(err.status)) return null;
    throw err;
  }
  const p = body?.session?.principal ?? body;
  const tenant = asText(p?.project_id ?? p?.projectId);
  const id = asText(p?.principal_id ?? p?.principalId ?? p?.id);
  if (!tenant || authTenantId(tenant) !== config.organization || !id || !/^usr_[a-zA-Z0-9_-]{8,80}$/.test(id) || (p.state != null && p.state !== 'active') || p.guest === true) return null;
  return { id, name: asText(p.display_name ?? p.displayName), email: asText(p.primary_email ?? p.primaryEmail), emailVerified: (p.primary_email_verified ?? p.primaryEmailVerified) === true };
}

export async function session(request: Request, config: Config) {
  const token = readAuthSessionCookie(request, { legacyCookies: legacySessionCookies })?.value;
  if (!token) return null;
  const user = await verifySessionUser(config, request, token);
  return user ? { user, token } : null;
}

export async function moneyAccessToken(request: Request, config: Config, token: string) {
  const body = await authCall(config, request, '/v1/client/token', token, {});
  const jwt = asText(body?.access_token);
  if (!jwt || !/^[\w-]+\.[\w-]+\.[\w-]+$/.test(jwt)) throw new HttpError(502, 'auth_unavailable');
  return jwt;
}
