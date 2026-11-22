import { createMiddleware } from '@solidjs/start/middleware';
import { isCanonicalHost, noindexTag } from './lib/indexing';
import { signInAlias } from './lib/sign-in-flow';
import { safeNext } from './lib/safe-next';

export default createMiddleware({ onRequest({ request, response }) {
  response.headers.set('X-Content-Type-Options', 'nosniff');
  response.headers.set('X-Frame-Options', 'DENY');
  response.headers.set('Referrer-Policy', 'strict-origin-when-cross-origin');
  if (!isCanonicalHost(request.headers)) response.headers.set('X-Robots-Tag', noindexTag);
  // Consent and sign-in render request-specific state in the initial HTML.
  const url = new URL(request.url);
  const path = url.pathname.replace(/\/+$/, '');
  if ((request.method === 'GET' || request.method === 'HEAD') && signInAlias(path)) {
    const next = url.searchParams.get('next');
    const query = next ? '?' + new URLSearchParams({ next: safeNext(next) }) : '';
    return new Response(null, { status: 308, headers: { Location: '/sign-in' + query, 'Cache-Control': 'private, no-store' } });
  }
  if (path === '/cli-auth' || path === '/sign-in') response.headers.set('Cache-Control', 'private, no-store');
} });
