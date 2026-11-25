export function showsArticleBridge({ canonical, noindex }: { canonical: string; noindex: boolean }) {
  if (noindex) return false;
  return canonical === '/docs' || canonical === '/vs' || /^\/(?:docs|vs|tasks|use-cases)\/.+/.test(canonical);
}
