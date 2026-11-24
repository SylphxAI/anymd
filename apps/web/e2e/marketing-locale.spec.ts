import { readFile } from 'node:fs/promises';
import { expect, test } from '@playwright/test';

const origin = 'https://anymd.sylphx.com';

test.describe('crawler-visible locale metadata', () => {
  test.use({ javaScriptEnabled: false });

  test('emits exact reciprocal alternates, a clean canonical and localized content', async ({ page, request }) => {
    for (const [route, canonical, en, tw] of [
      ['/?utm_source=fixture', '/', '/', '/zh-TW'],
      ['/zh-TW/?utm_source=fixture', '/zh-TW', '/', '/zh-TW'],
      ['/zh-Hant', '/zh-TW', '/', '/zh-TW'],
      ['/zh-Hant/pricing?currency=TWD', '/zh-TW/pricing', '/pricing', '/zh-TW/pricing'],
      ['/pricing?currency=EUR', '/pricing', '/pricing', '/zh-TW/pricing'],
      ['/zh-TW/pricing/?currency=TWD&session_id=fixture', '/zh-TW/pricing', '/pricing', '/zh-TW/pricing'],
      ['/pro?currency=EUR', '/pricing', '/pricing', '/zh-TW/pricing'],
      ['/studio', '/studio', '/studio', null],
      ['/playground', '/studio', '/studio', null],
      ['/docs/cli', '/docs/cli', '/docs/cli', null],
      ['/vs/markdownify-mcp', '/vs/markdownify-mcp', '/vs/markdownify-mcp', null],
      ['/tasks', '/tasks', '/tasks', null],
      ['/privacy', '/privacy', '/privacy', null],
    ] as const) {
      const response = await page.goto(route);
      expect(response?.status(), route).toBe(200);
      const rawHead = (await response!.text()).match(/<head(?:\s[^>]*)?>([\s\S]*?)<\/head>/)?.[1] ?? '';
      expect(rawHead.match(/rel="canonical"/g), route).toHaveLength(1);
      const alternates = [['en', origin + en], ...(tw ? [['zh-TW', origin + tw]] : []), ['x-default', origin + en]];
      expect(rawHead.match(/hreflang=/g), route).toHaveLength(alternates.length);
      await expect(page.locator('head link[rel="canonical"]')).toHaveAttribute('href', origin + canonical);
      expect(await page.locator('head link[rel="alternate"][hreflang]').evaluateAll(nodes =>
        nodes.map(node => [node.getAttribute('hreflang'), node.getAttribute('href')]))).toEqual(alternates);
      if (canonical.startsWith('/zh-TW')) {
        await expect(page.locator('[lang="zh-TW"] main')).toBeVisible();
        await expect(page.getByRole('heading', { level: 1 })).toContainText(canonical.endsWith('pricing') ? '免費轉檔，按需升級' : '讀取文件，');
      }
    }
    for (const [path, status] of [['/sign-in', 200], ['/recover', 200], ['/zh-TW/does-not-exist', 404]] as const) {
      const response = await page.goto(path);
      expect(response?.status(), path).toBe(status);
      await expect(page.locator('head link[rel="alternate"][hreflang]')).toHaveCount(0);
    }
    const sitemap = await (await request.get('/sitemap.xml')).text();
    for (const path of ['/zh-TW', '/zh-TW/pricing']) expect(sitemap).toContain('<loc>' + origin + path + '</loc>');
  });
});

test('switches languages and shows priced plans and readable examples at this viewport', async ({ page }) => {
  await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=GB\n' }));
  await page.goto('/');
  const consent = page.locator('.cookie-panel');
  await expect(consent).toBeVisible();
  for (const [path, tryStudio, openStudio, demo, third, output] of [
    ['/', 'Try Studio Free', 'Open Studio', 'Conversion examples', 'Video frame', 'Frame citation'],
    ['/zh-TW', '免費試用 Studio', '開啟 Studio', '轉換範例', '影片影格', '影格引用'],
  ] as const) {
    if (path === '/zh-TW') await page.getByRole('link', { name: '繁體中文', exact: true }).click();
    await expect(page).toHaveURL(path);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
    for (const name of [tryStudio, openStudio]) {
      const cta = page.getByRole('link', { name, exact: true });
      await expect(cta).toHaveAttribute('href', '/studio');
      // No scrolling or dismissing the bar: the first-visit viewport must hit the link.
      expect(await cta.evaluate(el => {
        const { x, y, width, height } = el.getBoundingClientRect();
        return el.contains(document.elementFromPoint(x + width / 2, y + height / 2));
      }), name).toBe(true);
    }
    const tablist = page.getByRole('tablist', { name: demo });
    await tablist.getByRole('tab', { name: 'PDF', exact: true }).focus();
    await page.keyboard.press('ArrowRight');
    await expect(page.getByRole('tabpanel').locator('pre')).toContainText('<!-- slide 2 -->');
    await tablist.getByRole('tab', { name: third, exact: true }).click();
    await expect(page.getByRole('tabpanel').locator('pre')).toContainText(output);
    await expect(page.getByRole('tabpanel').locator('pre')).toContainText('00:01.100');
    await page.evaluate(() => window.scrollTo(0, 0));
  }
  await page.getByRole('link', { name: /選購 Anymd Pro/ }).click();
  await expect(page).toHaveURL('/zh-TW/pricing');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('免費轉檔，按需升級');
  await expect(page.getByText('Bought Pro before account checkout?')).toHaveCount(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
  const currency = page.getByLabel('Billing currency');
  await expect(currency.locator('option[value=TWD]')).toHaveText('TWD · NT$959');
  await currency.selectOption('TWD');
  await expect(page.getByText('NT$959', { exact: true })).toBeVisible();
  await page.getByRole('link', { name: 'English', exact: true }).click();
  await expect(page).toHaveURL('/pricing?currency=TWD');
  await expect(page.getByLabel('Billing currency')).toHaveValue('TWD');
  await expect(page.locator('head link[rel="canonical"]')).toHaveCount(1);
  await expect(page.locator('head link[rel="canonical"]')).toHaveAttribute('href', origin + '/pricing');
});

test('downloads the actual local conversion without an upload', async ({ page }) => {
  const writes: string[] = [];
  page.on('request', request => { if (!['GET', 'HEAD'].includes(request.method())) writes.push(request.url()); });
  await page.goto('/studio');
  await expect(page.getByRole('button', { name: 'Download .md', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: 'Sample document' }).click();
  const output = page.getByLabel('Markdown output');
  await expect(output).toContainText('Field Notes');
  await expect(page.getByRole('button', { name: 'Copy Markdown', exact: true })).toBeVisible();
  const transfer = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Download .md', exact: true }).click();
  const download = await transfer;
  expect(download.suggestedFilename()).toBe('brief.md');
  expect(await readFile((await download.path())!, 'utf8')).toBe(await output.textContent());
  expect(writes).toEqual([]);
});
