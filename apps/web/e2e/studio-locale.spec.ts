import { readFile } from 'node:fs/promises';
import { expect, test } from '@playwright/test';

const tw = '/zh-TW/studio';

test('Traditional Chinese Studio converts, copies, downloads and finds a quote entirely in the browser', async ({ page }) => {
  const errors: string[] = [];
  const writes: string[] = [];
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => { if (!['GET', 'HEAD'].includes(request.method())) writes.push(request.url()); });
  await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  const response = await page.goto(tw);
  expect(response?.ok()).toBe(true);
  expect(response?.headers()['content-security-policy']).toContain("default-src 'none'");
  expect(response?.headers().vary?.toLowerCase()).toContain('sec-gpc');
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-TW');
  await expect(page).toHaveTitle('轉檔工作室 · Anymd');
  await page.getByRole('button', { name: '拒絕選用項目' }).click();
  const studio = page.getByRole('main');
  await expect(studio.getByRole('heading', { level: 1 })).toHaveText('轉檔工作室');
  await expect(studio.getByText('將文件拖曳到這裡')).toBeVisible();
  for (const name of ['開啟檔案', '範例文件', '範例 PDF', '範例影片']) {
    await expect(studio.getByRole('button', { name, exact: true })).toBeVisible();
  }
  const overflow = () => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(await overflow()).toBe(0);
  await studio.getByRole('button', { name: '範例文件', exact: true }).click();
  const output = studio.getByLabel('Markdown 輸出');
  await expect(output).toContainText('Field Notes');
  await expect(studio.getByText(/^1 節 · 約 [\d,]+ 個 token · 473 B → 298 B$/)).toBeVisible();
  const markdown = await output.textContent();
  await studio.getByRole('button', { name: '複製 Markdown' }).click();
  await expect(studio.getByRole('button', { name: '已複製' })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(markdown);
  const transfer = page.waitForEvent('download');
  await studio.getByRole('button', { name: '下載 .md' }).click();
  const download = await transfer;
  expect(download.suggestedFilename()).toBe('brief.md');
  expect(await readFile((await download.path())!, 'utf8')).toBe(markdown);
  const quote = studio.getByLabel('文件中的引文');
  const find = studio.getByRole('button', { name: '查找引文' });
  await quote.fill('No such exact quote');
  await find.click();
  await expect(studio.getByRole('status')).toHaveText('未在同一頁、投影片或章節內找到完全相符的文字。');
  await quote.fill('Field Notes');
  await find.click();
  await expect(studio.getByRole('status')).toHaveText('找到引文：文件。');
  await studio.getByText('查看 sample.pdf 中已核對的引文', { exact: true }).click();
  await expect(studio.getByRole('img', { name: /^第 1 頁局部畫面/ })).toBeVisible();
  await expect(studio.getByText('引文已逐字核對')).toBeVisible();
  await expect(studio).not.toContainText(/Document Studio|Open a file|Sample document|Copy Markdown|Download \.md|Find quote|Exact quote verified|PRO PREVIEW/i);
  expect(await overflow()).toBe(0);
  await studio.locator('input[type=file]').setInputFiles({ name: 'empty.txt', mimeType: 'text/plain', buffer: Buffer.alloc(0) });
  await expect(studio.getByRole('alert')).toContainText('不超過 32 MB');
  await expect(output).toHaveCount(0);
  expect(writes).toEqual([]);
  expect(errors).toEqual([]);
});

test('Studio switches both ways and carries permitted campaign fields into localized pricing', async ({ page }) => {
  await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
  const tags: string[] = [];
  await page.route(url => /(?:^|\.)(?:googletagmanager|google-analytics|googleadservices|doubleclick)\.(?:com|net)$/.test(url.hostname), route => {
    tags.push(route.request().url());
    return route.fulfill({ contentType: 'application/javascript', body: '' });
  });
  const campaign = 'gclid=fixture&utm_source=locale';
  await page.goto('/zh-Hant/studio?' + campaign + '&unrelated=do-not-forward#studio-cite-heading');
  await expect(page).toHaveURL(tw + '?' + campaign + '&unrelated=do-not-forward#studio-cite-heading');
  const english = page.getByRole('link', { name: 'English', exact: true });
  const optIn = page.getByRole('button', { name: '允許分析與廣告', exact: true });
  await expect(optIn).toBeEnabled();
  await expect(english).toHaveAttribute('href', '/studio?' + campaign);
  await english.click();
  await expect(page).toHaveURL('/studio?' + campaign);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Document Studio.');
  await page.getByRole('link', { name: '繁體中文', exact: true }).click();
  await expect(page).toHaveURL(tw + '?' + campaign);
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-TW');
  const cta = page.getByRole('link', { name: /購買 Anymd Pro/ });
  const pricing = '/zh-TW/pricing?entry_surface=studio_cite_preview&' + campaign;
  await expect(cta).toHaveAttribute('href', pricing);
  await cta.click();
  await expect(page).toHaveURL(pricing);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('免費轉檔，按需升級');
  const proofLink = page.getByRole('link', { name: '查看引文座標' });
  await expect(proofLink).toHaveAttribute('href', tw + '?sample=pdf&' + campaign + '#studio-cite-heading');
  await expect(optIn).toBeEnabled();
  expect(tags).toEqual([]);
  expect(await page.evaluate(() => localStorage.getItem('anymd.signal.v1.touch'))).toBeNull();
  await optIn.click();
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('anymd.signal.v1.touch') ?? 'null')?.touch))
    .toEqual({ gclid: 'fixture', utm_source: 'locale' });
  await expect(proofLink).toHaveAttribute('href', tw + '?sample=pdf#studio-cite-heading');
});
