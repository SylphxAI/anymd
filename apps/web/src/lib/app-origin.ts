export const canonicalOrigin = 'https://anymd.sylphx.com';
const stagingOrigin = 'https://web-16b143.sylphx.app';

export function publicOrigin(headers: Headers) {
  // Ingress can replace Host upstream. Only accept this app's public hosts,
  // never a forwarded chain or another tenant's *.sylphx.app hostname.
  const publicHost = (headers.get('x-forwarded-host') ?? headers.get('host') ?? '').toLowerCase().replace(/:443$/, '');
  const origin = 'https://' + publicHost;
  return origin === stagingOrigin || origin === canonicalOrigin ? origin : null;
}
