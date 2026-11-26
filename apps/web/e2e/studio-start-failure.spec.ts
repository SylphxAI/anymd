import { expect, test } from '@playwright/test';

const live = Boolean(process.env.PLAYWRIGHT_BASE_URL);
const modulePath = '/wasm/anymd_wasm.js';
const enginePaths = ['/studio-worker.js', modulePath, '/wasm/anymd_wasm_bg.wasm'];
const locales = [
  {
    path: '/studio', sample: 'Sample PDF', output: 'Markdown output',
    start: 'The browser converter could not start. Try again or use the native CLI.',
    read: 'Couldn’t read this document. Check its format, or try the same file with the native CLI.',
  },
  {
    path: '/zh-TW/studio', sample: '範例 PDF', output: 'Markdown 輸出',
    start: '瀏覽器無法啟動轉檔工具。請再試一次，或改用本機 CLI。',
    read: '無法讀取這份文件。請確認檔案格式，或用本機 CLI 開啟同一個檔案。',
  },
];

test.use({
  serviceWorkers: 'block',
  extraHTTPHeaders: { 'Sec-GPC': '1' },
  ...(live ? { trace: 'off', screenshot: 'off', video: 'off' } : {}),
});

for (const locale of locales) {
  test(locale.path + ' distinguishes converter startup from an unreadable PDF and recovers on retry', async ({ page, context, hasTouch, baseURL }) => {
    const origin = new URL(baseURL!).origin;
    let failedImports = 0;
    await context.route(origin + modulePath, route => {
      failedImports++;
      return route.abort('failed');
    }, { times: 1 });

    const engineRevisions = new Map<string, Set<string | undefined>>();
    if (live) {
      context.on('response', response => {
        const url = new URL(response.url());
        if (url.origin !== origin || !enginePaths.includes(url.pathname)) return;
        const revisions = engineRevisions.get(url.pathname) ?? new Set();
        revisions.add(response.headers()['x-sylphx-revision-sha']);
        engineRevisions.set(url.pathname, revisions);
      });
    }

    const document = await page.goto(locale.path);
    expect(document?.status()).toBe(200);
    const studio = page.locator('.studio');
    const sample = studio.getByRole('button', { name: locale.sample, exact: true });
    const alert = studio.getByRole('alert');
    const output = studio.getByLabel(locale.output, { exact: true });
    const trySample = () => hasTouch ? sample.tap() : sample.click();
    await expect(sample).toBeEnabled();

    await trySample();
    await expect(alert).toHaveText(locale.start, { timeout: 15_000 });
    await expect(alert).toBeVisible();
    expect(failedImports).toBe(1);
    await expect(output).toHaveCount(0);

    // A nonempty, truncated PDF reaches the real reader, not the size check.
    await studio.locator('input[type=file]').setInputFiles({
      name: 'truncated.pdf', mimeType: 'application/pdf', buffer: Buffer.from('%PDF-1.7\n'),
    });
    await expect(alert).toHaveText(locale.read, { timeout: 15_000 });
    await expect(alert).toBeVisible();
    await expect(output).toHaveCount(0);

    await trySample();
    await expect(output).toContainText('Quarterly Field Report', { timeout: 15_000 });
    await expect(output).toBeVisible();
    await expect(output).toContainText('<!-- page 1 -->');
    await expect(output).toContainText('<!-- page 2 -->');
    await expect(alert).toHaveCount(0);
    expect(failedImports).toBe(1);

    if (live) {
      const revision = document?.headers()['x-sylphx-revision-sha'];
      expect(revision).toMatch(/^[a-f0-9]{40}$/);
      for (const path of enginePaths) expect(engineRevisions.get(path), path).toEqual(new Set([revision]));
      console.log('Studio and converter revision: %s', revision);
    }
  });
}
