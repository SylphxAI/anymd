import { createMiddleware } from '@solidjs/start/middleware';
import { isCanonicalHost, noindexTag } from './lib/indexing';
import { signInAlias } from './lib/sign-in-flow';
import { safeNext } from './lib/safe-next';

export default createMiddleware({ onRequest({ request, response }) {
  response.headers.set('X-Content-Type-Options', 'nosniff');
  response.headers.set('X-Frame-Options', 'DENY');
  response.headers.set('Referrer-Policy', 'strict-origin-when-cross-origin');
  if (!isCanonicalHost(request.headers)) response.headers.set('X-Robots-Tag', noindexTag);
  // Keep auth and payment proof URLs out of outbound referrers.
  const url = new URL(request.url);
  const path = url.pathname.replace(/\/+$/, '');
  if (['/sign-in', '/recover', '/pro/thanks'].includes(path)) response.headers.set('Referrer-Policy', 'strict-origin');
  if ((request.method === 'GET' || request.method === 'HEAD') && signInAlias(path)) {
    const next = url.searchParams.get('next');
    const query = next ? '?' + new URLSearchParams({ next: safeNext(next) }) : '';
    return new Response(null, { status: 308, headers: { Location: '/sign-in' + query, 'Cache-Control': 'private, no-store' } });
  }
  if (['/cli-auth', '/sign-in', '/pro/thanks'].includes(path)) response.headers.set('Cache-Control', 'private, no-store');
} });
