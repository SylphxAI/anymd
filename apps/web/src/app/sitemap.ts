import type { MetadataRoute } from 'next';
import { canonicalDocPath, documents } from '@/lib/docs';
import { canonicalHostname } from '@/lib/indexing';

export default function sitemap(): MetadataRoute.Sitemap {
  const paths = new Set(['/', '/studio', '/pricing', '/docs', ...documents.map(canonicalDocPath)]);
  return [...paths].sort().map((path) => ({ url: 'https://' + canonicalHostname + path }));
}
