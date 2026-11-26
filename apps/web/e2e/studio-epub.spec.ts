import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { expect, test } from '@playwright/test';

// For a read-only public run, use PLAYWRIGHT_BASE_URL with --reporter=line --last-failed-file /dev/null.
const live = Boolean(process.env.PLAYWRIGHT_BASE_URL);
const filename = 'epub-markitdown.epub';
const fixture = new URL('../../../bench/files/' + filename, import.meta.url);
const wasmPath = '/wasm/anymd_wasm_bg.wasm';
const enginePaths = ['/studio-worker.js', '/wasm/anymd_wasm.js', wasmPath];
const toc = 'Test EPUB Document';
const first = 'Chapter 1: Test Content';
const second = 'Chapter 2: More Content';

test.use({
  serviceWorkers: 'block',
  extraHTTPHeaders: { 'Sec-GPC': '1' },
  ...(live ? { trace: 'off', screenshot: 'off', video: 'off' } : {}),
});

test('Studio keeps the authored EPUB contents once before both chapters without sending the file', async ({ page, context, baseURL, hasTouch }) => {
  const buffer = await readFile(fixture);
  expect(createHash('sha256').update(buffer).digest('hex')).toBe('33e65f39f54bd24f74d5f333d24213a68c62ee2f2854d12f8c4c59b15a4031a7');

  const origin = new URL(baseURL!).origin;
  const unexpected: string[] = [];
  let fileSelected = false;
  await context.routeWebSocket('**/*', socket => {
    unexpected.push('WebSocket');
    void socket.close();
  });
  await context.route('**/*', async route => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.origin === origin && !url.search && request.method() === 'GET' && request.postData() === null) {
      if (fileSelected && enginePaths.includes(url.pathname)) return route.continue();
      if (!fileSelected) {
        if (url.pathname === '/cdn-cgi/trace') return route.fulfill({ body: 'loc=US\n' });
        if (['/studio', '/favicon.svg', '/api/auth/session'].includes(url.pathname) || url.pathname.startsWith('/_build/assets/')) {
          return route.continue();
        }
      }
    }
    // Block before delivery; don't copy potential document data into diagnostics.
    unexpected.push(request.method() + ' ' + request.resourceType());
    return route.abort();
  });

  const document = await page.goto('/studio');
  expect(document?.status()).toBe(200);
  const studio = page.locator('.studio');
  const open = studio.getByRole('button', { name: 'Open a file', exact: true });
  await expect(open).toBeEnabled();
  const [chooser] = await Promise.all([page.waitForEvent('filechooser'), hasTouch ? open.tap() : open.click()]);
  fileSelected = true;
  const [response] = await Promise.all([
    context.waitForEvent('response', {
      predicate: response => response.url() === origin + wasmPath,
      timeout: 15_000,
    }),
    chooser.setFiles({ name: filename, mimeType: 'application/epub+zip', buffer }),
  ]);
  expect(response.ok()).toBe(true);
  expect(response.headers()['content-type']).toContain('application/wasm');
  if (live) {
    const pageRevision = document?.headers()['x-sylphx-revision-sha'];
    const wasmRevision = response.headers()['x-sylphx-revision-sha'];
    console.log('Studio revision: %s; WASM revision: %s', pageRevision, wasmRevision);
    expect(pageRevision).toMatch(/^[a-f0-9]{40}$/);
    expect(wasmRevision).toBe(pageRevision);
  }

  const output = studio.getByLabel('Markdown output', { exact: true });
  await expect(output).toBeVisible({ timeout: 15_000 });
  const markdown = (await output.textContent())!;
  // Ignore source marker numbering, which moves when the navigation is retained.
  const chapters = markdown.split(/^<!-- chapter \d+: .* -->$/m).slice(1).map(unit => unit.trim());
  expect(chapters.slice(-2)).toEqual([
    '# ' + first + '\n\nThis is a **test** paragraph with some formatting.\n\nIt includes:\n\n- A bullet point\n- Another point',
    '# ' + second + "\n\nHere's a second chapter with a *different* style.\n\n> This is a blockquote for testing.",
  ]);
  await studio.getByRole('button', { name: 'Preview', exact: true }).click();
  const preview = studio.getByRole('region', { name: 'Markdown preview', exact: true });
  await expect(preview).toBeVisible();
  expect(unexpected, 'No network requests except the page and its own static assets and converter').toEqual([]);
  const outline = preview.getByRole('heading').or(preview.locator('ol').getByRole('listitem'));
  expect(await outline.allTextContents(), 'The visible EPUB TOC must appear exactly once, before both chapters').toEqual([
    toc, 'Chapter 1', 'Chapter 2', first, second,
  ]);
  for (const item of await outline.all()) await expect(item).toBeVisible();
});
