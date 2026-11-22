import { canonicalOrigin } from './app-origin';
import { signInAlias } from './sign-in-flow';

export function safeNext(value: unknown): string {
  if (typeof value !== 'string' || value.length > 1800 || !value.startsWith('/') || /[\\\u0000-\u001f]/.test(value)) return '/dashboard';
  const url = new URL(value, canonicalOrigin);
  // Allow page navigation, not an API call or a second redirect.
  if (url.origin !== canonicalOrigin || signInAlias(url.pathname) || /^\/(?:api|sign-in)(?:\/|$)/i.test(url.pathname) || /%2f|%5c/i.test(url.pathname)) return '/dashboard';
  return url.pathname + url.search + url.hash;
}
