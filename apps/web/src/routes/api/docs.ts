import { renderDoc } from '@/lib/docs-source';

export function GET({ request }: { request: Request }) {
  const query = new URL(request.url).searchParams;
  const slugs = query.getAll('slug');
  const legacy = query.getAll('legacy');
  const slug = slugs[0];
  if (slugs.length === 1 && slug.length <= 120 && /^[a-z0-9-]+(?:\/[a-z0-9-]+)?$/.test(slug)
    && legacy.length <= 1 && legacy.every(value => value === '0' || value === '1')
    && query.size === slugs.length + legacy.length) {
    const article = renderDoc(slug, legacy[0] === '1');
    if (article) return Response.json(article, { headers: { 'Cache-Control': 'public, max-age=60' } });
  }
  return Response.json({ error: 'not_found' }, { status: 404, headers: { 'Cache-Control': 'no-store' } });
}
