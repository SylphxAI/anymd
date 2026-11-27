import { expect, test } from '@playwright/test';

const cases = [
  ['/anymd', '/'],
  ['/anymd/', '/'],
  ['/anymd/index.html', '/'],
  ['/anymd/pro', '/pricing'],
  ['/anymd/pro.html', '/pricing'],
  ['/anymd/pricing', '/pricing'],
  ['/anymd/pricing.html', '/pricing'],
  ['/anymd/docs', '/docs'],
  ['/anymd/studio', '/studio'],
  ['/anymd/comparisons', '/vs'],
  ['/anymd/comparisons.html', '/vs'],
  ['/anymd/pro/thanks.html', '/pro/thanks'],
  ['/anymd/vs/index.html', '/vs'],
  ['/pricing.html', '/pricing'],
  ['/pro.html', '/pricing'],
  ['/index.html', '/'],
];

test('legacy Pages URLs return permanent redirects with exact local locations', async ({ request }) => {
  for (const [from, to] of cases) {
    const response = await request.get(from, { maxRedirects: 0 });
    expect(response.status(), from).toBe(301);
    expect(response.headers().location, from).toBe(to);
  }
});

test('GET and HEAD preserve the raw campaign query, including repeats and encoding', async ({ request }) => {
  const query = '?utm_source=docs+nav&gclid=Ab%2bc&tag=one&tag=two&next=https%3A%2F%2Fexample.invalid';
  for (const method of ['GET', 'HEAD']) {
    const response = await request.fetch('/anymd/pricing.html' + query, { method, maxRedirects: 0 });
    expect(response.status(), method).toBe(301);
    expect(response.headers().location, method).toBe('/pricing' + query);
  }
});

test('ambiguous legacy paths are rejected without a Location', async ({ request }) => {
  for (const path of ['/anymd//example.invalid', '/anymd/%2fexample.invalid', '/anymd/%5cexample.invalid']) {
    const response = await request.get(path, { maxRedirects: 0 });
    expect(response.status(), path).toBe(400);
    expect(response.headers().location, path).toBeUndefined();
  }
});

test('canonical pages, API and unrelated paths are not rewritten', async ({ request }) => {
  for (const path of ['/', '/pricing', '/pro/thanks', '/docs', '/studio', '/vs', '/api/health']) {
    const response = await request.get(path, { maxRedirects: 0 });
    expect(response.status(), path).toBe(200);
    expect(response.headers().location, path).toBeUndefined();
  }
  for (const path of ['/anymd-extra', '/not-a-legacy-page.html']) {
    const response = await request.get(path, { maxRedirects: 0 });
    expect(response.status(), path).toBe(404);
    expect(response.headers().location, path).toBeUndefined();
  }
  const post = await request.post('/anymd/pricing', { maxRedirects: 0 });
  expect(post.status()).not.toBe(301);
  expect(post.headers().location).toBeUndefined();
});
