import { expect, test } from '@playwright/test';

const studioChunk = /\/_build\/assets\/studio-page-[^/]+\.js$/;
const locales = [
  { path: '/studio', buttons: ['Open a file', 'Sample document', 'Sample PDF', 'Sample video'], loading: 'Loading Studio…', drop: 'Drop a document here', output: 'Markdown output', pricing: '/pricing' },
  { path: '/zh-TW/studio', buttons: ['開啟檔案', '範例文件', '範例 PDF', '範例影片'], loading: '正在載入轉檔工作室…', drop: '將文件拖曳到這裡', output: 'Markdown 輸出', pricing: '/zh-TW/pricing' },
];

for (const locale of locales) {
  test(locale.path + ' waits for Studio hydration, then converts on the first tap', async ({ page, hasTouch }) => {
    const chunk = Promise.withResolvers<void>();
    let sampleRequests = 0;
    page.on('request', request => {
      if (new URL(request.url()).pathname === '/samples/sample.pdf') sampleRequests++;
    });
    await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=US\n' }));
    await page.route('https://www.googletagmanager.com/**', route => route.fulfill({ contentType: 'application/javascript', body: '' }));
    // Hold the shared Studio module, not the page shell or its styles.
    await page.route(studioChunk, async route => {
      await chunk.promise;
      await route.continue();
    });

    const controls = page.locator('.studio-controls');
    const workspace = page.locator('.studio-workspace');
    const [open, doc, pdf, video] = locale.buttons.map(name => controls.getByRole('button', { name, exact: true }));
    const geometry = async () => [await controls.boundingBox(), await workspace.boundingBox()];
    let before;
    try {
      await Promise.all([page.waitForRequest(studioChunk), page.goto(locale.path, { waitUntil: 'commit' })]);
      for (const button of [open, doc, pdf, video]) await expect(button).toBeDisabled();
      await expect(controls).toHaveAttribute('aria-busy', 'true');
      await expect(workspace.getByText(locale.loading, { exact: true })).toBeVisible();
      await expect(page.locator('.cite-preview a[href*="entry_surface=studio_cite_preview"]')).toHaveAttribute('href', locale.pricing + '?entry_surface=studio_cite_preview');
      await expect(page.locator('header a.language-switch')).toHaveAttribute('href', locale.path === '/studio' ? '/zh-TW/studio' : '/studio');
      expect(sampleRequests).toBe(0);
      before = await geometry();
    } finally {
      chunk.resolve();
    }

    for (const button of [open, doc, pdf, video]) await expect(button).toBeEnabled();
    await expect(controls).toHaveAttribute('aria-busy', 'false');
    await expect(workspace.getByText(locale.drop, { exact: true })).toBeVisible();
    expect(await geometry()).toEqual(before);
    const sample = page.waitForResponse(response => new URL(response.url()).pathname === '/samples/sample.pdf', { timeout: 5_000 });
    const [response] = await Promise.all([sample, hasTouch ? pdf.tap() : pdf.click()]);
    expect(response.ok()).toBe(true);
    expect(sampleRequests).toBe(1);
    // A cold converter also fetches and initializes WASM after the first tap.
    await expect(workspace.getByLabel(locale.output, { exact: true })).toContainText('Quarterly Field Report', { timeout: 15_000 });
    expect(sampleRequests).toBe(1);
  });
}
