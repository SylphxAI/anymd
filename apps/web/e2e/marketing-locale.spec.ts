import { readFile } from 'node:fs/promises';
import { test } from './fixture-test';
import { expect } from '@playwright/test';

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
      ['/studio?utm_source=fixture', '/studio', '/studio', '/zh-TW/studio'],
      ['/zh-TW/studio?utm_source=fixture', '/zh-TW/studio', '/studio', '/zh-TW/studio'],
      ['/playground', '/studio', '/studio', '/zh-TW/studio'],
      ['/docs/cli', '/docs/cli', '/docs/cli', null],
      ['/vs/markdownify-mcp', '/vs/markdownify-mcp', '/vs/markdownify-mcp', null],
      ['/tasks', '/tasks', '/tasks', null],
      ['/privacy', '/privacy', '/privacy', null],
    ] as const) {
      const response = await page.goto(route);
      expect(response?.status(), route).toBe(200);
      const html = await response!.text();
      const lang = canonical.startsWith('/zh-TW') ? 'zh-TW' : 'en';
      expect(html.match(/<html[^>]*\blang="([^"]+)"/)?.[1], route).toBe(lang);
      await expect(page.locator('html')).toHaveAttribute('lang', lang);
      const rawHead = html.match(/<head(?:\s[^>]*)?>([\s\S]*?)<\/head>/)?.[1] ?? '';
      expect(rawHead.match(/rel="canonical"/g), route).toHaveLength(1);
      const alternates = [['en', origin + en], ...(tw ? [['zh-TW', origin + tw]] : []), ['x-default', origin + en]];
      expect(rawHead.match(/hreflang=/g), route).toHaveLength(alternates.length);
      await expect(page.locator('head link[rel="canonical"]')).toHaveAttribute('href', origin + canonical);
      expect(await page.locator('head link[rel="alternate"][hreflang]').evaluateAll(nodes =>
        nodes.map(node => [node.getAttribute('hreflang'), node.getAttribute('href')]))).toEqual(alternates);
      if (canonical.startsWith('/zh-TW')) {
        await expect(page.getByRole('main')).toBeVisible();
        const heading = canonical.endsWith('/studio') ? '轉檔工作室' : canonical.endsWith('/pricing') ? '免費轉檔，按需升級' : '讀取文件，';
        await expect(page.getByRole('heading', { level: 1 })).toContainText(heading);
      }
    }
    for (const [path, status] of [['/sign-in', 200], ['/recover', 200], ['/zh-TW/does-not-exist', 404]] as const) {
      const response = await page.goto(path);
      expect(response?.status(), path).toBe(status);
      await expect(page.locator('head link[rel="alternate"][hreflang]')).toHaveCount(0);
    }
    const sitemap = await (await request.get('/sitemap.xml')).text();
    for (const path of ['/zh-TW', '/zh-TW/pricing', '/studio', '/zh-TW/studio']) expect(sitemap).toContain('<loc>' + origin + path + '</loc>');
    expect(sitemap).not.toContain('/zh-Hant/studio');
    for (const method of ['GET', 'HEAD']) {
      const alias = await request.fetch('/zh-Hant/studio?gclid=fixture&UTM_unknown=a%2Bb&utm_source=zh%20tw', { method, maxRedirects: 0 });
      expect(alias.status()).toBe(308);
      expect(alias.headers().location).toBe('/zh-TW/studio?gclid=fixture&UTM_unknown=a%2Bb&utm_source=zh%20tw');
    }
  });
});

test('switches languages and shows priced plans and readable examples at this viewport', async ({ page }) => {
  await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=GB\n' }));
  await page.goto('/');
  const consent = page.locator('.cookie-panel');
  await expect(consent).toBeVisible();
  for (const [path, tryStudio, openStudio, demo, third, output] of [
    ['/', 'Try Studio Free', 'Open Studio', 'Conversion examples', 'Video frame', 'Frame citation'],
    ['/zh-TW', '免費試用轉檔工作室', '開啟轉檔工作室', '轉換範例', '影片影格', '影格引用'],
  ] as const) {
    if (path === '/zh-TW') await page.getByRole('link', { name: '繁體中文', exact: true }).click();
    await expect(page).toHaveURL(path);
    await expect(page.locator('html')).toHaveAttribute('lang', path === '/' ? 'en' : 'zh-TW');
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
    for (const name of [tryStudio, openStudio]) {
      const cta = page.getByRole('link', { name, exact: true });
      await expect(cta).toHaveAttribute('href', path === '/' ? '/studio' : '/zh-TW/studio');
      // No scrolling or dismissing the bar: the first-visit viewport must hit the link.
      expect(await cta.evaluate(el => {
        const { x, y, width, height } = el.getBoundingClientRect();
        return el.contains(document.elementFromPoint(x + width / 2, y + height / 2));
      }), name).toBe(true);
    }
    const tablist = page.getByRole('tablist', { name: demo });
    const tabs = tablist.getByRole('tab');
    await expect(tabs).toHaveCount(3);
    const lineBands = await tabs.locator('span').evaluateAll(labels => labels.map(label => {
      const text = document.createRange();
      text.selectNodeContents(label);
      return Array.from(text.getClientRects(), rect => Math.round(rect.top));
    }));
    expect(lineBands).toEqual([[lineBands[0][0]], [lineBands[0][0]], [lineBands[0][0]]]);
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
  const currency = page.getByLabel('結帳幣別');
  await expect(currency.locator('option[value=TWD]')).toHaveText('TWD · NT$959');
  await currency.selectOption('TWD');
  await expect(page.getByText('NT$959', { exact: true })).toBeVisible();
  await page.getByRole('link', { name: 'English', exact: true }).click();
  await expect(page).toHaveURL('/pricing?currency=TWD');
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await expect(consent.getByRole('heading')).toHaveText('Cookie settings');
  await expect(page.getByLabel('Billing currency')).toHaveValue('TWD');
  await expect(page.locator('head link[rel="canonical"]')).toHaveCount(1);
  await expect(page.locator('head link[rel="canonical"]')).toHaveAttribute('href', origin + '/pricing');
});

for (const prefix of ['/zh-TW', '/zh-Hant']) {
  test(prefix + ' localizes checkout, Studio links and all three consent choices', async ({ page }) => {
    await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=GB\n' }));
    for (const path of [prefix, prefix + '/pricing']) {
      await page.goto(path);
      await expect(page.locator('html')).toHaveAttribute('lang', 'zh-TW');
      const header = page.getByRole('banner');
      await expect(header.getByRole('link', { name: '登入', exact: true })).toBeVisible();
      const mobileMenu = header.getByLabel('開啟導覽選單');
      if (await mobileMenu.isVisible()) await mobileMenu.click();
      await expect(header.getByRole('link', { name: '轉檔工作室', exact: true })).toHaveAttribute('href', '/zh-TW/studio');
      await expect(header).not.toContainText('（英文）');
      if (await mobileMenu.isVisible()) await mobileMenu.click();

      const consent = page.getByRole('complementary', { name: 'Cookie 設定' });
      await expect(consent).toContainText('選用分析與廣告。');
      await expect(consent.getByRole('link', { name: '隱私權說明' })).toHaveAttribute('href', '/privacy');
      await expect(consent.getByRole('button')).toHaveCount(3);
      for (const name of ['拒絕選用項目', '僅限分析', '允許分析與廣告']) {
        await expect(consent.getByRole('button', { name, exact: true })).toBeEnabled();
      }
      if (path === prefix) {
        for (const name of ['免費試用轉檔工作室', '開啟轉檔工作室']) {
          await expect(page.getByRole('link', { name, exact: true })).toHaveAttribute('href', '/zh-TW/studio');
        }
      } else {
        const pro = page.locator('.pro-plan');
        await expect(pro.getByRole('link', { name: '登入以購買 Pro', exact: true })).toBeVisible();
        await expect(pro.getByRole('link', { name: 'Pro 條款' })).toHaveAttribute('href', '/legal/pro-terms');
        await expect(pro).toContainText('詳閱 Pro 條款以了解授權、交付與取消說明。');
        await expect(pro.getByLabel('結帳幣別').locator('option[value=TWD]')).toHaveText('TWD · NT$959');
        await consent.getByRole('button', { name: '拒絕選用項目' }).click();
        await expect(consent).toHaveCount(0);
        expect(await page.evaluate(() => localStorage.getItem('anymd.signal.v1.consent'))).toBe('denied');
        const settings = page.getByRole('button', { name: 'Cookie 設定', exact: true });
        await settings.click();
        await expect(consent.getByRole('button', { name: '拒絕選用項目' })).toBeFocused();
        await consent.getByRole('button', { name: '關閉，不變更設定' }).click();
        await expect(settings).toBeFocused();
      }
    }
  });
}

test('returns from sign-in to the localized purchase and keeps checkout states in Traditional Chinese', async ({ page }) => {
  await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=GB\n' }));
  await page.goto('/zh-Hant/pricing?currency=TWD');
  await page.getByRole('button', { name: '拒絕選用項目' }).click();
  await page.getByRole('link', { name: '登入以購買 Pro', exact: true }).click();
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  await page.getByRole('link', { name: 'Continue with Google' }).click();
  await expect(page).toHaveURL('/zh-TW/pricing?entry_surface=pricing&currency=TWD');
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-TW');
  const header = page.getByRole('banner');
  await header.getByText('帳號', { exact: true }).click();
  await expect(header.getByRole('link', { name: '帳號總覽', exact: true })).toHaveAttribute('href', '/dashboard');
  await expect(header.getByRole('button', { name: '登出', exact: true })).toBeVisible();
  await header.getByText('帳號', { exact: true }).click();
  await expect(page.getByLabel('結帳幣別')).toHaveValue('TWD');
  const buy = page.getByRole('button', { name: '前往安全結帳', exact: true });
  await expect(buy).toBeDisabled();
  await page.getByRole('checkbox', { name: /我要求在付款後立即取得授權/ }).check();
  await page.route('**/api/anymd/checkout', async route => {
    expect(route.request().postDataJSON()).toMatchObject({ currency: 'TWD', consent: true, entrySurface: 'pricing' });
    await expect(page.getByRole('button', { name: '正在開啟結帳…' })).toBeDisabled();
    await route.fulfill({ status: 503, json: { code: 'billing_unavailable' } });
  });
  await buy.click();
  await expect(page.getByRole('alert')).toContainText('目前無法開啟結帳');
  await expect(buy).toBeEnabled();
  await header.getByText('帳號', { exact: true }).click();
  await header.getByRole('button', { name: '登出', exact: true }).click();
  await expect(page).toHaveURL('/zh-TW');
  await expect(header.getByRole('link', { name: '登入', exact: true })).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-TW');
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
