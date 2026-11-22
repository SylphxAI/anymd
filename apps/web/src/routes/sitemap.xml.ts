import { canonicalDocPath, documents } from '@/lib/docs-source';
import { canonicalOrigin } from '@/lib/app-origin';

export function GET() {
  const paths = new Set(['/', '/studio', '/pricing', '/docs', '/tasks', '/vs/markdownify-mcp', ...documents.map(canonicalDocPath)]);
  const entries = [...paths].sort().map((path) => '  <url><loc>' + canonicalOrigin + path + '</loc></url>').join('\n');
  return new Response('<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n' + entries + '\n</urlset>\n', {
    headers: { 'Content-Type': 'application/xml; charset=utf-8' },
  });
}
