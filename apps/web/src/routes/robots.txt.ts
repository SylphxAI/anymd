import type { APIEvent } from '@solidjs/start/server';
import { canonicalHostname, isCanonicalHost } from '@/lib/indexing';

export function GET({request}: APIEvent) {
  const rules = isCanonicalHost(request.headers)
    ? `Allow: /\n\nSitemap: https://${canonicalHostname}/sitemap.xml\n`
    : 'Disallow: /\n';
  return new Response(`User-agent: *\n${rules}`, {
    headers: { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store' },
  });
}
