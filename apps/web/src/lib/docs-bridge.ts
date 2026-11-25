import { knownProCtaSurface, type ProCtaSurface } from './entry-surface';

export function articleBridgeSurface(canonical: string): ProCtaSurface {
  if (canonical === '/vs') return 'seo_vs-hub';
  const [, section, slug] = canonical.split('/');
  if (!slug || canonical !== '/' + section + '/' + slug) return 'article_bridge';
  const candidate = section === 'vs' ? 'seo_vs-' + slug
    : section === 'use-cases' ? 'seo_' + slug
    : section === 'tasks' ? 'task_' + slug.replaceAll('-', '_') : undefined;
  return knownProCtaSurface(candidate) ?? 'article_bridge';
}

export function inlineProCtaSurface(href: string | null) {
  const prefix = '/pricing?';
  if (!href?.startsWith(prefix)) return;
  const search = href.slice(prefix.length).split('#', 1)[0];
  const values = new URLSearchParams(search).getAll('entry_surface');
  if (values.length === 1) return knownProCtaSurface(values[0]);
}

export function showsArticleBridge({ canonical, noindex }: { canonical: string; noindex: boolean }) {
  if (noindex) return false;
  return canonical === '/docs' || canonical === '/vs' || /^\/(?:docs|vs|tasks|use-cases)\/.+/.test(canonical);
}
