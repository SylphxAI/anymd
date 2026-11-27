import { guideAliasTarget } from './docs-source';

// Clean up the top-level HTML aliases emitted by scripts/build-pages-redirect.py.
const oldHtmlPages = new Set([
  '/index.html', '/pricing.html', '/docs.html', '/playground.html',
  '/pro.html', '/recover.html', '/vs.html', '/vision.html',
]);

// null means this is not a legacy URL; false means the legacy path is ambiguous.
export function legacyRedirect(url: URL): string | false | null {
  const { pathname } = url;
  const underLegacyRoot = pathname === '/anymd' || pathname.startsWith('/anymd/');
  if (!underLegacyRoot && !oldHtmlPages.has(pathname)) return null;

  const path = underLegacyRoot ? pathname.slice('/anymd'.length) : pathname;
  // Location must be a local absolute path, never an authority or an ambiguous path.
  if (path.startsWith('//') || /\\|%(?:2f|5c|0[0-9a-f]|1[0-9a-f]|7f)/i.test(path)) return false;
  const clean = path.replace(/\/+$/, '').replace(/(?:\/index)?\.html$/, '').replace(/\/+$/, '');
  const target = clean === '/pro' ? '/pricing' : clean === '/comparisons' ? '/vs' : guideAliasTarget(clean) ?? (clean || '/');
  if (!target.startsWith('/') || new URL(target, url).pathname !== target) return false;
  // Don't reserialize URLSearchParams: preserve campaign data, order and encoding.
  return target + url.search;
}
