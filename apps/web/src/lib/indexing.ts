export const canonicalHostname = 'anymd.sylphx.com';
export const noindexTag = 'noindex, nofollow';

export function isCanonicalHost(headers: Headers): boolean {
  // The ingress supplies the original host when it replaces Host upstream.
  const host = headers.get('x-forwarded-host')?.split(',', 1)[0] ?? headers.get('host');
  return host?.trim().toLowerCase().replace(/:\d+$/, '') === canonicalHostname;
}
