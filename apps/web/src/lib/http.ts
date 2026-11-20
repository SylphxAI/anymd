import 'server-only';

export const canonicalOrigin = 'https://anymd.sylphx.com';
const local = new Set(['localhost', '127.0.0.1', '[::1]']);

export function appOrigin(request: Request) {
  // Next standalone sees the container hostname in Request.url. The platform
  // ingress supplies the public Host. Never reflect arbitrary caller hosts.
  if (process.env.ANYMD_APP_ORIGIN) {
    const url = new URL(process.env.ANYMD_APP_ORIGIN);
    if (url.protocol === 'https:' || (url.protocol === 'http:' && local.has(url.hostname))) return url.origin;
    throw new HttpError(503, 'invalid_environment');
  }
  const host = request.headers.get('host') ?? '';
  if (/^[a-z0-9-]+(?:\.[a-z0-9-]+)*\.sylphx\.app$/i.test(host)) return `https://${host}`;
  if (/^(?:localhost|127\.0\.0\.1|\[::1\])(?::[0-9]{1,5})?$/.test(host)) return `http://${host}`;
  return canonicalOrigin;
}

export function safeNext(value: unknown): string {
  if (typeof value !== 'string' || value.length > 1800 || !value.startsWith('/') || /[\\\u0000-\u001f]/.test(value)) return '/dashboard';
  const url = new URL(value, canonicalOrigin);
  // Allow page navigation, not an API call or a second redirect.
  if (url.origin !== canonicalOrigin || /^\/(?:api|sign-in)(?:\/|$)/i.test(url.pathname) || /%2f|%5c/i.test(url.pathname)) return '/dashboard';
  return url.pathname + url.search + url.hash;
}

export class HttpError extends Error {
  constructor(readonly status: number, readonly code: string, readonly retryAfter?: string) { super(code); }
}

export async function sameOriginJson(request: Request): Promise<Record<string, unknown>> {
  if (request.headers.get('origin') !== appOrigin(request) || request.headers.get('sec-fetch-site') === 'cross-site') throw new HttpError(403, 'invalid_origin');
  if (request.headers.get('content-type')?.split(';')[0].trim().toLowerCase() !== 'application/json') throw new HttpError(415, 'json_required');
  const reader = request.body?.getReader();
  let text = '';
  let size = 0;
  const decoder = new TextDecoder();
  while (reader) {
    const { value, done } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > 32_768) { await reader.cancel(); throw new HttpError(413, 'invalid_request'); }
    text += decoder.decode(value, { stream: true });
  }
  try {
    const data = JSON.parse(text + decoder.decode());
    if (data && typeof data === 'object' && !Array.isArray(data)) return data;
  } catch { /* no upstream call on malformed JSON */ }
  throw new HttpError(400, 'invalid_request');
}

export function json(data: unknown, status = 200, headers: HeadersInit = {}) {
  return Response.json(data, { status, headers: { 'Cache-Control': 'private, no-store', ...headers } });
}

export function failure(error: unknown) {
  if (error instanceof HttpError) return json({code: error.code}, error.status, error.retryAfter ? {'Retry-After': error.retryAfter} : {});
  return json({code:'upstream_unavailable'}, 502);
}

export function redirect(location: string) {
  return new Response(null, { status: 303, headers: { Location: location, 'Cache-Control': 'private, no-store', 'Referrer-Policy': 'no-referrer' } });
}

export const asText = (value: unknown): string | null => typeof value === 'string' && value.trim() ? value.trim() : null;
export function field(data: Record<string, unknown>, key: string, pattern: RegExp) {
  const value = data[key];
  if (typeof value !== 'string' || !pattern.test(value)) throw new HttpError(400, 'invalid_request');
  return value;
}
