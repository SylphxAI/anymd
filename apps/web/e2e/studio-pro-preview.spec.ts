import { createHash } from 'node:crypto';
import { expect, test } from '@playwright/test';

test('Studio shows source-backed Pro proof without gating the free PDF quote search', async ({ page }, testInfo) => {
  const failures: string[] = [];
  page.on('console', message => { if (message.type() === 'error') failures.push(message.text()); });
  page.on('pageerror', error => failures.push(error.message));
  page.on('requestfailed', request => failures.push(`${request.url()}: ${request.failure()?.errorText}`));
  page.on('response', response => { if (response.status() >= 400) failures.push(`${response.status()} ${response.url()}`); });

  // The local production server is not behind Cloudflare. Exercise the EU consent flow without analytics.
  await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
  const route = await page.goto('/studio');
  expect(route?.ok()).toBe(true);
  await page.getByRole('button', { name: 'Reject optional' }).click();

  const preview = page.getByRole('region', { name: 'Verify a quote on the exact PDF page crop' });
  const toggle = preview.getByText('See the verified quote in sample.pdf', { exact: true });
  const crop = preview.getByRole('img', { name: /Page 1 crop highlighting/ });
  const checkWidth = async () => {
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()?.width ?? 0);
    expect(await preview.evaluate(element => element.scrollWidth)).toBeLessThanOrEqual(await preview.evaluate(element => element.clientWidth));
  };
  await expect(toggle).toBeVisible();
  await expect(crop).toBeHidden();
  await checkWidth();
  await toggle.focus();
  await toggle.press('Enter');
  await expect(crop).toBeVisible();
  await expect(preview.getByText('Exact quote verified', { exact: true })).toBeVisible();
  const image = crop.locator('img');
  await expect(image).toHaveJSProperty('naturalWidth', 790);
  await expect(image).toHaveJSProperty('naturalHeight', 182);
  await checkWidth();
  await preview.screenshot({ path: testInfo.outputPath('studio-cite-preview.png') });

  await preview.getByText('View bounding box and PDF SHA-256', { exact: true }).click();
  const evidence = preview.getByLabel('Citation evidence JSON');
  await expect(evidence).toBeVisible();
  const proof = JSON.parse(await evidence.innerText());
  expect(proof).toMatchObject({ page: 1, quote: 'Quarterly Field Report', verdict: 'verified_exact' });
  const { left, right, top, bottom } = proof.bbox;
  const rect = await crop.locator('rect').evaluate((element: SVGGraphicsElement) => {
    const { x, y, width, height } = element.getBBox();
    return { x, y, width, height };
  });
  expect(rect.x).toBeCloseTo(left - 15);
  expect(rect.y).toBeCloseTo(792 - top - 27);
  expect(rect.width).toBeCloseTo(right - left);
  expect(rect.height).toBeCloseTo(top - bottom);
  await checkWidth();
  await preview.screenshot({ path: testInfo.outputPath('studio-cite-json.png') });
  await toggle.click();
  await expect(crop).toBeHidden();

  const pdfResponse = page.waitForResponse('**/samples/sample.pdf');
  await page.getByRole('button', { name: 'Sample PDF' }).click();
  const pdf = await pdfResponse;
  expect(pdf.ok()).toBe(true);
  expect(createHash('sha256').update(await pdf.body()).digest('hex')).toBe(proof.sha256);
  await expect(page.getByLabel('Markdown output')).toContainText(proof.quote, { timeout: 15_000 });
  await page.getByLabel('Quote from the document').fill(proof.quote);
  await page.getByRole('button', { name: 'Find quote' }).click();
  await expect(page.getByText(/Found in.*page 1/i)).toBeVisible();
  await checkWidth();

  const cta = preview.getByRole('link', { name: 'Get Anymd Pro — $29 once', exact: true });
  await expect(cta).toHaveAttribute('href', '/pricing?entry_surface=studio_cite_preview');
  await cta.click();
  await expect(page).toHaveURL('/pricing?entry_surface=studio_cite_preview');
  await expect(page.getByRole('heading', { name: 'Anymd Pro', exact: true })).toBeVisible();
  expect(failures).toEqual([]);
});
