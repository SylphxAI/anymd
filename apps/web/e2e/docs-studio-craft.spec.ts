import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';
import { studioAsset } from './studio-assets';

test('Studio exports are in the first viewport above the consent bar immediately after PDF conversion', async ({ page }, info) => {
  await page.route('**/cdn-cgi/trace', route => route.fulfill({body:'loc=DE\n'}));
  await page.goto('/studio');
  const bar = page.getByRole('complementary', {name:'Cookie settings'});
  await expect(bar).toBeVisible();
  await expect(page.getByRole('button', {name:'Open a file'})).toHaveClass(/button-primary/);
  await page.getByRole('button', {name:'Sample PDF'}).click();
  const stats = page.getByText(/^2 pages · ~[\d,]+ tokens · 44\.3 KB → 1\.2 KB$/);
  const copy = page.getByRole('button', {name:'Copy Markdown', exact:true});
  const download = page.getByRole('button', {name:'Download .md', exact:true});
  await expect(stats).toBeVisible();
  expect(await page.evaluate(() => scrollY)).toBe(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
  const dock = (await bar.boundingBox())!;
  for (const target of [copy, download, stats]) {
    await expect(target).toBeInViewport({ratio:1});
    const bounds = (await target.boundingBox())!;
    expect(bounds.y + bounds.height).toBeLessThan(dock.y);
  }
  await page.screenshot({path:info.outputPath('studio-first-viewport.png')});
});

const pdfCases = [
  {
    route: '/studio', open: 'Open a file', sample: 'Sample PDF', output: 'Markdown output', preview: 'Preview',
    previewOutput: 'Markdown preview', copy: 'Copy Markdown', copied: 'Copied', download: 'Download .md',
    quote: 'Quote from the document', find: 'Find quote', found: 'Found in page 2.', pages: ['page 1', 'page 2'],
    partial: 'Partial conversion: No selectable text found on page 1.', empty: 'No selectable text found in this PDF.',
    ocr: 'local OCR instructions',
  },
  {
    route: '/zh-TW/studio', open: '開啟檔案', sample: '範例 PDF', output: 'Markdown 輸出', preview: '預覽',
    previewOutput: 'Markdown 預覽', copy: '複製 Markdown', copied: '已複製', download: '下載 .md',
    quote: '文件中的引文', find: '查找引文', found: '找到引文：第 2 頁。', pages: ['第 1 頁', '第 2 頁'],
    partial: '部分轉檔結果：第 1 頁未找到可選取的文字。', empty: '這份 PDF 沒有可選取的文字。',
    ocr: '本機 OCR 操作說明',
  },
];
const mixedText = 'This page has a real text layer over its background picture.';
const mixedMarkdown = '---\nsource: scanned-page.pdf\npages: 2\n---\n\n<!-- page 1 -->\n\n\n\n<!-- page 2 -->\n\n' + mixedText + '\n';
const ocrPath = '/docs/cli#local-document-ocr';

for (const locale of pdfCases) {
  test(locale.route + ' discloses partial PDF text without changing output or sending the file', async ({ page, context }, info) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await page.addInitScript(() => localStorage.setItem('anymd.cookie-settings.v1', 'reject'));
    await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
    const response = await page.goto(locale.route);
    expect(response?.ok()).toBe(true);
    if (process.env.PLAYWRIGHT_BASE_URL) {
      const rev = response?.headers()['x-sylphx-revision-sha'];
      expect(rev).toMatch(/^[a-f0-9]{40}$/);
      console.log(JSON.stringify({ studio: response?.url(), revision: rev, project: info.project.name }));
    }
    const origin = new URL(page.url()).origin;
    await expect(page.getByRole('button', { name: locale.open })).toBeEnabled();
    const unexpected: string[] = [];
    context.on('request', request => {
      const url = new URL(request.url());
      const converter = studioAsset(origin, request.url());
      const publicSample = url.origin === origin && url.pathname === '/samples/sample.pdf' && !url.search;
      if (request.postData() !== null || !['GET', 'HEAD'].includes(request.method()) ||
        (['fetch', 'xhr'].includes(request.resourceType()) && !converter && !publicSample) || url.href.includes('scanned-page.pdf')) {
        unexpected.push(request.method() + ' ' + request.url());
      }
    });

    const input = page.locator('input[type=file]');
    const status = page.getByRole('status');
    const output = page.getByLabel(locale.output);
    const ocr = page.getByRole('link', { name: locale.ocr, exact: true });
    await input.setInputFiles(fileURLToPath(new URL('../../../test/fixtures/scanned-page.pdf', import.meta.url)));
    const partial = status.filter({ hasText: locale.partial });
    await expect(partial).toBeInViewport({ ratio: 1 });
    await expect(partial.getByRole('link', { name: locale.ocr })).toHaveAttribute('href', ocrPath);
    await expect.poll(() => output.textContent()).toBe(mixedMarkdown);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
    await page.screenshot({ path: info.outputPath('studio-pdf-partial.png') });

    await page.getByRole('button', { name: locale.preview, exact: true }).click();
    const preview = page.getByRole('region', { name: locale.previewOutput });
    for (const marker of locale.pages) await expect(preview.getByText(marker, { exact: true })).toBeVisible();
    await expect(preview.getByText(mixedText, { exact: true })).toBeVisible();
    await expect(partial).toBeVisible();
    await page.getByRole('button', { name: locale.copy, exact: true }).click();
    await expect(page.getByRole('button', { name: locale.copied, exact: true })).toBeVisible();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(mixedMarkdown);
    const completed = page.waitForEvent('download');
    await page.getByRole('button', { name: locale.download, exact: true }).click();
    const download = await completed;
    expect(download.suggestedFilename()).toBe('scanned-page.md');
    expect(await readFile((await download.path())!, 'utf8')).toBe(mixedMarkdown);
    await page.getByLabel(locale.quote).fill(mixedText);
    await page.getByRole('button', { name: locale.find }).click();
    await expect(status.filter({ hasText: locale.found })).toBeVisible();

    await page.getByRole('button', { name: locale.sample, exact: true }).click();
    await expect(output).toContainText('Quarterly Field Report');
    await expect(ocr).toHaveCount(0);
    await expect(page.getByRole('button', { name: locale.download, exact: true })).toBeVisible();

    await input.setInputFiles({ name: 'image-only.pdf', mimeType: 'application/pdf', buffer: imageOnlyPdf() });
    const empty = status.filter({ hasText: locale.empty });
    await expect(empty).toBeVisible();
    await expect(partial).toHaveCount(0);
    await expect(empty.getByRole('link', { name: locale.ocr })).toHaveAttribute('href', ocrPath);
    await expect(output).toHaveCount(0);
    for (const label of [locale.copy, locale.download, locale.find]) {
      await expect(page.getByRole('button', { name: label, exact: true })).toHaveCount(0);
    }
    expect(unexpected).toEqual([]);
    await ocr.press('Enter');
    await expect(page).toHaveURL(ocrPath);
    await expect(page.getByRole('heading', { name: 'Local document OCR', exact: true })).toBeInViewport();
  });
}

test('copies code exactly and exposes Python API in the reference nav', async ({ page }) => {
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.addInitScript(() => localStorage.setItem('anymd.cookie-settings.v1', 'reject'));
  for (const path of ['/docs', '/docs/cli', '/docs/tools', '/docs/python']) {
    await page.goto(path);
    await expect(page.getByRole('navigation', { name: 'Documentation' }).getByRole('link', { name: 'Python API' })).toHaveAttribute('href', '/docs/python');
    const code = page.locator('article pre > code');
    const copy = page.getByRole('button', { name: 'Copy code', exact: true });
    await expect(code.first()).toBeVisible();
    await expect(copy).toHaveCount(await code.count());
    await expect(copy.first()).toBeVisible();
    await copy.first().press('Enter');
    await expect(page.getByRole('button', { name: 'Copied', exact: true })).toBeVisible();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(await code.first().textContent());
  }
});

test('describes one activation flow and exposes distinct route descriptions', async ({ page }) => {
  for (const path of ['/pricing', '/docs/hosted', '/legal/pro-terms']) {
    await page.goto(path);
    await expect(page.getByRole('main')).toContainText('anymd login');
    await expect(page.getByRole('main')).not.toContainText('anymd pro buy');
  }
  const descriptions = new Set<string | null>();
  for (const path of ['/', '/docs', '/pricing']) {
    await page.goto(path);
    const description = page.locator('head meta[name=description]');
    await expect(description).toHaveAttribute('content', /Markdown|Install/);
    descriptions.add(await description.getAttribute('content'));
  }
  expect(descriptions.size).toBe(3);
});

// A one-page PDF with a one-pixel raster image and no text operators.
function imageOnlyPdf() {
  const drawing = 'q 90 0 0 90 5 5 cm /Im0 Do Q\n';
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>',
    '<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /ASCIIHexDecode /Length 3 >>\nstream\n00>\nendstream',
    '<< /Length ' + drawing.length + ' >>\nstream\n' + drawing + 'endstream',
  ];
  let pdf = '%PDF-1.4\n';
  const offsets = objects.map((object, index) => {
    const offset = pdf.length;
    pdf += (index + 1) + ' 0 obj\n' + object + '\nendobj\n';
    return offset;
  });
  return Buffer.from(pdf + 'xref\n0 6\n0000000000 65535 f \n' + offsets.map(offset => String(offset).padStart(10, '0') + ' 00000 n \n').join('') +
    'trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n' + pdf.length + '\n%%EOF\n');
}

test('filters the canonical guide index in the browser', async ({ page }) => {
  await page.goto('/docs');
  const search = page.getByRole('searchbox', { name: 'Search documentation' });
  await search.fill('  pyTHon  ');
  const results = page.getByRole('region', { name: 'Filter documentation' });
  await expect(results.locator('a[href="/docs/python"]')).toBeVisible();
  await search.fill('qqq-no-such-guide');
  await expect(results.getByRole('status')).toHaveText('No guides found. Try a different term.');
  await search.clear();
  await expect(results.getByRole('status')).toHaveCount(0);
});
