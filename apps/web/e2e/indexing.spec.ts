import { expect, test } from '@playwright/test';

const cases = [
  { name: 'staging', host: 'web-16b143.sylphx.app', indexable: false },
  { name: 'staging with port', host: 'web-16b143.sylphx.app:443', indexable: false },
  { name: 'forwarded staging', host: '0.0.0.0:3000', forwarded: 'web-16b143.sylphx.app', indexable: false },
  { name: 'localhost', host: 'localhost:3000', indexable: false },
  { name: 'forwarded staging takes precedence', host: 'anymd.sylphx.com', forwarded: 'web-16b143.sylphx.app:443, anymd.sylphx.com', indexable: false },
  { name: 'canonical lookalike', host: 'anymd.sylphx.com.example.test', indexable: false },
  { name: 'canonical', host: 'anymd.sylphx.com', indexable: true },
  { name: 'forwarded canonical', host: '0.0.0.0:3000', forwarded: 'ANYMD.SYLPHX.COM:443, internal.example.test', indexable: true },
];

for (const { name, host, forwarded, indexable } of cases) {
  test(`${name} gets the expected crawl policy`, async ({ request }) => {
    const headers: Record<string, string> = { host };
    if (forwarded) headers['x-forwarded-host'] = forwarded;
    const options = { headers, maxRedirects: 0 };
    const expectedTag = indexable ? undefined : 'noindex, nofollow';
    for (const path of ['/', '/studio', '/pricing', '/dashboard', '/docs', '/api/health']) {
      const response = await request.get(path, options);
      expect(response.status(), path).toBe(200);
      expect.soft(response.headers()['x-robots-tag'], path).toBe(expectedTag);
    }
    const sitemap = await request.get('/sitemap.xml', options);
    expect(sitemap.status()).toBe(200);
    expect(sitemap.headers()['content-type']).toContain('application/xml');
    expect.soft(sitemap.headers()['x-robots-tag'], '/sitemap.xml').toBe(expectedTag);
    const xml = await sitemap.text();
    const urls = [...xml.matchAll(/<loc>([^<]+)<\/loc>/g)].map((match) => new URL(match[1]));
    expect(urls.every((url) => url.origin === 'https://anymd.sylphx.com')).toBe(true);
    const paths = urls.map((url) => url.pathname);
    expect(paths).toEqual(expect.arrayContaining(['/', '/studio', '/pricing', '/docs', '/docs/cli', '/vision', '/vs', '/vs/docling', '/legal/privacy']));
    expect(new Set(paths).size).toBe(paths.length);
    for (const alias of ['/pro', '/playground', '/guide/cli', '/docs/getting-started', '/vs/index']) expect(paths).not.toContain(alias);
    expect(paths.some((path) => /^\/(sign-in|account|billing|dashboard|recover|cli-auth|api)(?:\/|$)/.test(path))).toBe(false);
    const robots = await request.get('/robots.txt', options);
    expect(robots.status()).toBe(200);
    expect.soft(robots.headers()['x-robots-tag'], '/robots.txt').toBe(expectedTag);
    expect.soft(robots.headers()['cache-control']).toContain('no-store');
    expect(await robots.text()).toBe(indexable
      ? 'User-agent: *\nAllow: /\n\nSitemap: https://anymd.sylphx.com/sitemap.xml\n'
      : 'User-agent: *\nDisallow: /\n');
  });
}
