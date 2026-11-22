// These paths need a real document request rather than Solid Router navigation.
export function isBackendHref(href: string, base: string): boolean {
  try {
    const url = new URL(href, base);
    return /^https?:$/.test(url.protocol) && /^\/(?:api|auth|oauth)(?:\/|$)/i.test(url.pathname);
  } catch {
    return false;
  }
}
