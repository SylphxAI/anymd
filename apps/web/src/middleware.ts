import { createMiddleware } from '@solidjs/start/middleware';
import { isCanonicalHost, noindexTag } from './lib/indexing';
import { isSignInPath, signInAlias } from './lib/sign-in-flow';
import { safeNext } from './lib/safe-next';
import { legacyRedirect } from './lib/legacy-redirect';

export default createMiddleware({ onRequest({ request, response }) {
  response.headers.set('X-Content-Type-Options', 'nosniff');
  response.headers.set('X-Frame-Options', 'DENY');
  // SSR carries Sec-GPC into the browser even when the navigator API is absent.
  response.headers.append('Vary', 'Sec-GPC');
  response.headers.set('Referrer-Policy', 'strict-origin-when-cross-origin');
  if (!isCanonicalHost(request.headers)) response.headers.set('X-Robots-Tag', noindexTag);
  // Keep auth and payment proof URLs out of outbound referrers.
  const url = new URL(request.url);
  if (request.method === 'GET' || request.method === 'HEAD') {
    const legacy = legacyRedirect(url);
    if (legacy === false) return new Response(null, { status: 400 });
    if (legacy) return new Response(null, { status: 301, headers: { Location: legacy } });
  }
  const path = url.pathname.replace(/\/+$/, '');
  if (isSignInPath(path) || ['/recover', '/pro/thanks'].includes(path)) response.headers.set('Referrer-Policy', 'strict-origin');
  if ((request.method === 'GET' || request.method === 'HEAD') && signInAlias(path)) {
    const next = url.searchParams.get('next');
    const query = next ? '?' + new URLSearchParams({ next: safeNext(next) }) : '';
    return new Response(null, { status: 308, headers: { Location: '/sign-in' + query, 'Cache-Control': 'private, no-store' } });
  }
  if (isSignInPath(path) || ['/cli-auth', '/pro/thanks'].includes(path)) response.headers.set('Cache-Control', 'private, no-store');
} });
