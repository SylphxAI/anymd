import { readdirSync } from 'node:fs';
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

const guides = readdirSync(new URL('../../../docs/guide/', import.meta.url)).filter((file) => file.endsWith('.md')).map((file) => file.slice(0, -3));
const guideTarget = (slug: string) => slug === 'getting-started' ? '/docs' : '/docs/' + slug;

test('every legacy guide URL redirects once to its canonical doc', async ({ request }) => {
  expect(guides).toEqual(expect.arrayContaining(['getting-started', 'cli', 'hosted', 'video-timeline']));
  for (const slug of guides) {
    for (const from of ['/guide/' + slug, '/guide/' + slug + '/']) {
      const response = await request.get(from, { maxRedirects: 0 });
      expect(response.status(), from).toBe(308);
      expect(response.headers().location, from).toBe(guideTarget(slug));
    }
    const landing = await request.get(guideTarget(slug), { maxRedirects: 0 });
    expect(landing.status(), slug).toBe(200);
    expect(landing.headers().location, slug).toBeUndefined();
  }
  const old = await request.get('/anymd/guide/cli/index.html', { maxRedirects: 0 });
  expect(old.status()).toBe(301);
  expect(old.headers().location).toBe('/docs/cli');
});

test('legacy guide redirects keep the raw query for GET and HEAD only', async ({ request }) => {
  const query = '?utm_source=docs+nav&gclid=Ab%2bc&tag=one&tag=two&next=https%3A%2F%2Fexample.invalid';
  for (const method of ['GET', 'HEAD']) {
    const response = await request.fetch('/guide/cli' + query, { method, maxRedirects: 0 });
    expect(response.status(), method).toBe(308);
    expect(response.headers().location, method).toBe('/docs/cli' + query);
  }
  const post = await request.post('/guide/cli', { maxRedirects: 0 });
  expect(post.headers().location).toBeUndefined();
});

test('unknown guide paths stay 404 without a Location', async ({ request }) => {
  for (const path of ['/guide', '/guide/', '/guide/missing-doc', '/guide/CLI', '/guide/cli.html', '/guide/cli/extra', '/guide/index', '/guide/cli%2f..', '/guide/cli%2F']) {
    const response = await request.get(path, { maxRedirects: 0 });
    expect(response.status(), path).toBe(404);
    expect(response.headers().location, path).toBeUndefined();
  }
});

test('rendered docs link to canonical guide paths, not legacy aliases', async ({ request }) => {
  for (const path of ['/vision', '/tasks/build-video-timeline']) {
    const html = await (await request.get(path)).text();
    expect(html, path).not.toMatch(/href="\/guide\//);
  }
  expect(await (await request.get('/vision')).text()).toContain('href="/docs/hosted"');
});
