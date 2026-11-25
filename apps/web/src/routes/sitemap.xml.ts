import { canonicalDocPath, indexableDocuments } from '@/lib/docs-source';
import { canonicalOrigin } from '@/lib/app-origin';
import { locales, localizedPath } from '@/lib/marketing-locale';

export function GET() {
  const paths = new Set(['/', '/studio', '/pricing', '/docs', '/tasks', '/vs/markdownify-mcp', ...indexableDocuments.map(canonicalDocPath)]);
  const entries = [...new Set([...paths].flatMap(path => locales.map(locale => localizedPath(path, locale))))].sort().map((path) => '  <url><loc>' + canonicalOrigin + path + '</loc></url>').join('\n');
  return new Response('<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n' + entries + '\n</urlset>\n', {
    headers: { 'Content-Type': 'application/xml; charset=utf-8' },
  });
}
