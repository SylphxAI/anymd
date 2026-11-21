import { createMiddleware } from '@solidjs/start/middleware';
import { isCanonicalHost, noindexTag } from './lib/indexing';

export default createMiddleware({ onRequest({ request, response }) {
  response.headers.set('X-Content-Type-Options', 'nosniff');
  response.headers.set('X-Frame-Options', 'DENY');
  response.headers.set('Referrer-Policy', 'strict-origin-when-cross-origin');
  if (!isCanonicalHost(request.headers)) response.headers.set('X-Robots-Tag', noindexTag);
  // Consent and sign-in render request-specific state in the initial HTML.
  const path = new URL(request.url).pathname.replace(/\/+$/, '');
  if (path === '/cli-auth' || path === '/sign-in') response.headers.set('Cache-Control', 'private, no-store');
} });
