import { expect, test } from '@playwright/test';

const locales = [
  { path: '/studio', view: 'Output view', preview: 'Preview', output: 'Markdown output', pdf: 'Sample PDF', doc: 'Sample document', quote: 'Quote from the document', find: 'Find quote', marker: 'page 1', source: 'source' },
  { path: '/zh-TW/studio', view: '輸出檢視', preview: '預覽', output: 'Markdown 輸出', pdf: '範例 PDF', doc: '範例文件', quote: '文件中的引文', find: '查找引文', marker: '第 1 頁', source: '來源' },
];

for (const locale of locales) {
  test(locale.path + ' previews both samples without reconversion, layout shift or analytics', async ({ page }) => {
    const errors: string[] = [];
    const writes: string[] = [];
    const signal: string[] = [];
    let workers = 0;
    page.on('worker', () => { workers++; });
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    page.on('pageerror', error => errors.push(error.message));
    page.on('request', request => {
      if (!['GET', 'HEAD'].includes(request.method())) writes.push(request.url());
      if (request.url().endsWith('/v1/analytics/events:batch')) signal.push(request.url());
    });
    // US keeps the workbench uncovered without making a consent choice.
    await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=US\n' }));
    await page.route('https://www.googletagmanager.com/**', route => route.fulfill({ contentType: 'application/javascript', body: '' }));
    const response = await page.goto(locale.path);
    expect(response?.headers()['content-security-policy']).toMatch(/script-src [^;]*'nonce-[^']+'/);
    const studio = page.locator('.studio');
    const group = studio.getByRole('group', { name: locale.view });
    const raw = group.getByRole('button', { name: 'Markdown', exact: true });
    const preview = group.getByRole('button', { name: locale.preview, exact: true });
    const output = studio.getByLabel(locale.output, { exact: true });
    const pane = studio.locator('.studio-output-pane');
    const rendered = studio.locator('.studio-preview');
    const checkWidth = () => page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);

    for (const sample of [
      { button: locale.pdf, title: 'Quarterly Field Report', sizes: '44.3 KB → 1.2 KB', column: 'Region' },
      { button: locale.doc, title: 'Field Notes: Water Survey', sizes: '473 B → 298 B', column: 'Site' },
    ]) {
      await studio.getByRole('button', { name: sample.button, exact: true }).click();
      await expect(output).toContainText(sample.title);
      await expect(studio.locator('.studio-stats')).toContainText(sample.sizes);
      await expect(raw).toHaveAttribute('aria-pressed', 'true');
      const before = await output.textContent();
      const geometry = await pane.boundingBox();
      const converted = workers;
      await preview.focus();
      await page.keyboard.press('Enter');
      await expect(preview).toHaveAttribute('aria-pressed', 'true');
      await expect(preview).toBeFocused();
      await expect(rendered.getByRole('heading', { name: sample.title, exact: true })).toBeVisible();
      await expect(rendered.locator('dt').first()).toHaveText(locale.source);
      await expect(rendered.getByRole('columnheader', { name: sample.column })).toBeVisible();
      if (sample.button === locale.pdf) await expect(rendered.locator('.studio-marker').first()).toHaveText(locale.marker);
      await expect(rendered.locator('[style], a, img')).toHaveCount(0);
      expect(await pane.boundingBox()).toEqual(geometry);
      expect(await checkWidth()).toBe(0);
      await raw.click();
      await expect(output).toBeVisible();
      expect(await output.textContent()).toBe(before);
      expect(workers).toBe(converted);

      const quote = studio.getByLabel(locale.quote);
      await quote.fill(sample.title.toUpperCase());
      await studio.getByRole('button', { name: locale.find }).click();
      await expect(studio.locator('.studio-excerpts mark')).toHaveText(sample.title);
      await quote.fill('not in this file');
      await studio.getByRole('button', { name: locale.find }).click();
      await expect(studio.locator('.studio-excerpts')).toHaveCount(0);
    }
    if (locale.path === '/zh-TW') {
      const markdown = [
        '## 水質', '', '|Column One|Column Two|Column Three|Column Four|Column Five|Column Six|',
        '|-|-|-|-|-|-|', '|雨季|4|5|6|7|8|', '', '![Figure](https://example.invalid/preview-fixture.png)',
      ].join('\n');
      const external: string[] = [];
      await page.route('https://example.invalid/**', route => {
        external.push(route.request().url());
        return route.fulfill({ body: '' });
      });
      await studio.locator('input[type=file]').setInputFiles({ name: 'fixture.md', mimeType: 'text/markdown', buffer: Buffer.from(markdown) });
      await expect(output).toContainText('水質');
      await expect(studio.locator('.studio-stats')).toContainText('171 B → 213 B');
      await preview.click();
      await expect(rendered.getByRole('heading', { name: '水質' })).toBeVisible();
      await expect(rendered.getByText('![Figure](https://example.invalid/preview-fixture.png)', { exact: true })).toBeVisible();
      const table = rendered.locator('.studio-table');
      if (page.viewportSize()!.width === 390) {
        expect(await table.evaluate(el => el.scrollWidth)).toBeGreaterThan(await table.evaluate(el => el.clientWidth));
        await table.evaluate(el => { el.scrollLeft = el.scrollWidth; });
        expect(await table.evaluate(el => el.scrollLeft)).toBeGreaterThan(0);
      }
      expect(await checkWidth()).toBe(0);
      expect(external).toEqual([]);
    }
    expect(errors).toEqual([]);
    expect(writes).toEqual([]);
    expect(signal).toEqual([]);
  });
}
