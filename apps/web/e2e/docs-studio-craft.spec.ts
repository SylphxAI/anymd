import { expect, test } from '@playwright/test';

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

test('shows a useful OCR state and no empty exports or quote search for a scanned PDF', async ({ page }) => {
  await page.goto('/studio');
  await page.locator('input[type=file]').setInputFiles({ name: 'image-only.pdf', mimeType: 'application/pdf', buffer: imageOnlyPdf() });
  await expect(page.getByRole('status').filter({ hasText: 'No selectable text found in this PDF.' })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Use OCR in the native CLI' })).toHaveAttribute('href', '/docs/cli');
  for (const label of ['Copy Markdown', 'Download .md', 'Find quote']) await expect(page.getByRole('button', { name: label, exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: 'Sample document' }).click();
  await expect(page.getByLabel('Markdown output')).toContainText('Field Notes');
  await expect(page.getByRole('button', { name: 'Find quote' })).toBeVisible();
});

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
