const fields = ['gclid', 'gbraid', 'wbraid', 'utm_source', 'utm_medium', 'utm_campaign', 'utm_term', 'utm_content'] as const;

export function carryCampaign(href: string, search: string) {
  // Restrict to app paths, never OAuth, API, media URLs or protocol-relative links.
  if (!/^\/(?:[^/\\]|$)/.test(href) || /[\\\p{Cc}]/u.test(href) || !search) return href;
  const local = 'https://anymd.invalid';
  const destination = new URL(href, local);
  if (destination.origin !== local || /^\/(?:api|auth|oauth)(?:\/|$)/i.test(destination.pathname)) return href;
  const source = new URLSearchParams(search);
  const carry = new URLSearchParams();
  for (const field of fields) {
    const values = source.getAll(field);
    const value = values[0];
    if (values.length !== 1 || destination.searchParams.has(field)) continue;
    if (field.startsWith('utm_')
      ? value.trim() && !/[\p{Cc}\ufffd]/u.test(value) && new TextEncoder().encode(value).length <= 256
      : /^[A-Za-z0-9_.~-]{1,256}$/.test(value)) carry.set(field, value);
  }
  if (!carry.size) return href;
  const hashAt = href.indexOf('#');
  const beforeHash = hashAt < 0 ? href : href.slice(0, hashAt);
  const afterHash = hashAt < 0 ? '' : href.slice(hashAt);
  return beforeHash + (beforeHash.includes('?') ? '&' : '?') + carry + afterHash;
}
