
import { canonicalOrigin, publicOrigin } from './app-origin';

export { canonicalOrigin } from './app-origin';
const local = new Set(['localhost', '127.0.0.1', '[::1]']);

export function appOrigin(request: Request) {
  const host = request.headers.get('host') ?? '';
  const origin = publicOrigin(request.headers);
  if (origin) return origin;
  if (process.env.ANYMD_APP_ORIGIN) {
    const url = new URL(process.env.ANYMD_APP_ORIGIN);
    if (url.protocol === 'https:' || (url.protocol === 'http:' && local.has(url.hostname))) return url.origin;
    throw new HttpError(503, 'invalid_environment');
  }
  if (/^(?:localhost|127\.0\.0\.1|\[::1\])(?::[0-9]{1,5})?$/.test(host)) return `http://${host}`;
  return canonicalOrigin;
}

export { safeNext } from './safe-next';

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
