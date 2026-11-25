import { expect, test, type Page } from '@playwright/test';
import { signalChoiceKey } from '../src/lib/consent';

async function observe(page: Page) {
  const rpc: string[] = [], errors: string[] = [], forbidden: string[] = [], docs: string[] = [], telemetry: string[] = [];
  await page.route('**/_server*', route => {
    rpc.push(route.request().url());
    return route.abort();
  });
  await page.route('**/cdn-cgi/trace', route => route.fulfill({body:'loc=DE\n'}));
  await page.route(url => url.pathname === '/v1/analytics/events:batch' || /(?:^|\.)(?:googletagmanager|google-analytics|googleadservices|google|doubleclick)\.(?:com|net)$/.test(url.hostname), route => {
    telemetry.push(route.request().url());
    return route.abort();
  });
  page.on('request', request => {
    if (new URL(request.url()).pathname === '/api/docs') docs.push(request.url());
  });
  page.on('response', response => {
    if (response.status() === 403) forbidden.push(response.url());
  });
  page.on('pageerror', error => errors.push(error.message));
  return { rpc, errors, forbidden, docs, telemetry };
}

for (const [entry, label, menu] of [
  ['/', 'Docs', 'Open navigation'],
  ['/zh-TW/studio', '文件', '開啟導覽選單'],
]) {
  test(entry + ' opens Docs from the header without an RPC or full navigation', async ({ page, isMobile, baseURL }) => {
    const messages = await observe(page);
    await page.goto(entry);
    await expect(page.getByRole('complementary', {name: /Cookie/}).getByRole('button', {name: /Reject optional|拒絕選用項目/})).toBeEnabled();
    if (isMobile) await page.getByLabel(menu, {exact:true}).click();
    // A fresh document would lose this marker.
    await page.evaluate(() => Reflect.set(window, 'docsNavMarker', true));
    const response = page.waitForResponse(url => new URL(url.url()).pathname === '/api/docs');
    await page.locator('header').getByRole('link', {name:label, exact:true}).filter({visible:true}).click();
    await expect(page).toHaveURL('/docs');
    expect((await response).status()).toBe(200);
    const article = page.locator('article.prose');
    await expect(article.getByRole('heading', {name:'Getting started', exact:true})).toBeVisible();
    await expect(article).toContainText('Every MCP client runs the same command');
    expect(await page.evaluate(() => Reflect.get(window, 'docsNavMarker'))).toBe(true);
    expect(messages).toEqual({rpc:[], errors:[], forbidden:[], telemetry:[], docs:[baseURL + '/api/docs?slug=guide%2Fgetting-started']});
  });
}

test('rejecting optional cookies updates links in place without fetching or remounting the article', async ({ page }) => {
  const messages = await observe(page);
  const path = '/tasks/check-pdf-citation?gclid=REJ_TEST&utm_source=google';
  const response = await page.goto(path);
  expect(response?.status()).toBe(200);
  const article = page.locator('article.prose');
  const h1 = article.getByRole('heading', {level:1});
  await expect(h1).toHaveText('Check whether a PDF quote is on the cited page');
  const cta = article.getByRole('link', {name:'See Anymd Pro — US$29 one-time'});
  const base = '/pricing?entry_surface=task_check_pdf_citation';
  await expect(cta).toHaveAttribute('href', base + '&gclid=REJ_TEST&utm_source=google');
  const bridge = page.getByRole('region', {name:'Try a file of your own'});
  const studio = bridge.getByRole('link', {name:'Open Document Studio'});
  const pro = bridge.getByRole('link', {name:'Anymd Pro — $29 once'});
  await expect(studio).toHaveAttribute('href', '/studio?gclid=REJ_TEST&utm_source=google');
  await expect(pro).toHaveAttribute('href', base + '&gclid=REJ_TEST&utm_source=google');
  await h1.evaluate(node => node.dataset.retained = 'true');
  const text = await article.textContent();
  await page.getByRole('button', {name:'Reject optional', exact:true}).click();
  await expect(cta).toHaveAttribute('href', base);
  await expect(studio).toHaveAttribute('href', '/studio');
  await expect(pro).toHaveAttribute('href', base);
  await expect(h1).toHaveAttribute('data-retained', 'true');
  expect(await article.textContent()).toBe(text);
  await expect(page.locator('main a[href*="gclid="], main a[href*="utm_source="]')).toHaveCount(0);
  await expect(page).toHaveURL(path);
  expect(messages).toEqual({rpc:[], errors:[], forbidden:[], telemetry:[], docs:[]});
});

for (const {path, title, proof, header} of [
  {path:'/tasks/check-pdf-citation', title:'Check whether a PDF quote is on the cited page', proof:'Make a PDF you can reproduce', header:true},
  {path:'/use-cases/pdf-and-office-to-markdown-mcp', title:'Convert PDF and Office files to Markdown', proof:'96.4 overall score', header:true},
  {path:'/tasks/check-pdf-citation', title:'Check whether a PDF quote is on the cited page', proof:'Make a PDF you can reproduce', header:false},
]) {
  test(path + ' remains readable under ' + (header ? 'HTTP + browser' : 'browser-only') + ' GPC', async ({ page }) => {
    const messages = await observe(page);
    if (header) await page.setExtraHTTPHeaders({'Sec-GPC':'1'});
    await page.addInitScript(key => {
      localStorage.setItem(key, 'granted');
      Object.defineProperty(navigator, 'globalPrivacyControl', {value:true});
    }, signalChoiceKey);
    const response = await page.goto(path + '?gclid=GPC_TEST&utm_source=google');
    expect(response?.status()).toBe(200);
    if (header) expect(await response!.text()).not.toMatch(/<a\b[^>]*href="[^"]*gclid=GPC_TEST/);
    const article = page.locator('article.prose[data-interactive=true]');
    await expect(article.getByRole('heading', {level:1})).toHaveText(title);
    await expect(article).toContainText(proof);
    await expect(page.locator('a[href*="gclid="], a[href*="utm_source=google"]')).toHaveCount(0);
    expect(await page.evaluate(() => Reflect.get(window, 'dataLayer') ?? [])).toEqual([]);
    expect(messages).toEqual({rpc:[], errors:[], forbidden:[], telemetry:[], docs:[]});
  });
}

test('public docs JSON is campaign-neutral, validates inputs and keeps legacy aliases', async ({ request }) => {
  const canonical = await request.get('/api/docs?slug=guide%2Fpython');
  const legacy = await request.get('/api/docs?slug=python&legacy=1');
  expect(canonical.status()).toBe(200);
  expect(canonical.headers()['cache-control']).toBe('public, max-age=60');
  const document = await canonical.json();
  expect(document).toMatchObject({canonical:'/docs/python', html:expect.stringContaining('<h1>')});
  expect(await legacy.json()).toEqual(document);
  for (const query of ['slug=guide%2F..', 'slug=missing-doc', 'slug=python&slug=cli', 'slug=python&legacy=2', 'slug=guide%2Fpython&gclid=NOT_A_DOC_PARAMETER']) {
    const missing = await request.get('/api/docs?' + query);
    expect(missing.status()).toBe(404);
    expect(missing.headers()['cache-control']).toBe('no-store');
    expect(await missing.json()).toEqual({error:'not_found'});
  }
});

test('docs SPA shows a recoverable message if the public JSON request fails', async ({ page, isMobile }) => {
  await observe(page);
  await page.route('**/api/docs?**', route => route.fulfill({status:503}));
  await page.goto('/');
  await expect(page.getByRole('button', {name:'Reject optional', exact:true})).toBeEnabled();
  if (isMobile) await page.getByLabel('Open navigation', {exact:true}).click();
  await page.locator('header').getByRole('link', {name:'Docs', exact:true}).filter({visible:true}).click();
  await expect(page.getByRole('heading', {name:'Documentation unavailable', exact:true})).toBeVisible();
});
