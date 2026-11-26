import { expect, test, type Locator, type Page } from '@playwright/test';
import { signalChoiceKey } from '../src/lib/consent';

const campaign = 'gclid=fixture&utm_source=organic-fixture&utm_medium=cpc&utm_campaign=PDF+Proof';
const query = '?' + campaign + '&sample=video&entry_surface=cli_pdf&unrelated=drop-me';
const fragment = '#studio-cite-heading';
const citation = '/studio?sample=pdf&' + campaign + fragment;
const links = [
  ['Sample PDF', '/studio?sample=pdf&' + campaign],
  ['PDF citation proof', citation],
  ['HTML brief', '/studio?sample=document&' + campaign],
] as const;

async function expectNoOverflow(page: Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
  expect(await page.evaluate(() => document.documentElement.clientWidth)).toBe(page.viewportSize()!.width);
}

async function expectTapTarget(link: Locator) {
  await expect(link).toBeVisible();
  const box = (await link.boundingBox())!;
  expect(box.width).toBeGreaterThanOrEqual(44);
  expect(box.height).toBeGreaterThanOrEqual(44);
  const splitWords = await link.evaluate(node => {
    const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
    const range = document.createRange();
    const split: string[] = [];
    for (let text = walker.nextNode(); text; text = walker.nextNode()) {
      for (const { 0: word, index } of (text.textContent ?? '').matchAll(/\S+/g)) {
        range.setStart(text, index);
        range.setEnd(text, index + word.length);
        if (range.getClientRects().length !== 1) split.push(word);
      }
    }
    return split;
  });
  expect(splitWords).toEqual([]);
  return box;
}

for (const fixture of [
  { path: '/docs', surface: 'article_bridge' },
  { path: '/vs/docling', surface: 'seo_vs-docling', proof: 'PDF citation proof' },
  { path: '/tasks/check-pdf-citation', surface: 'task_check_pdf_citation' },
  { path: '/vs/markdownify-mcp', surface: 'seo_vs-markdownify-mcp', proof: 'Inspect a cited PDF in Studio' },
]) {
  test(fixture.path + ' offers local Studio samples before consent', async ({ page, context, hasTouch }) => {
    const errors: string[] = [];
    const batches: string[] = [];
    const writes: string[] = [];
    context.on('request', request => {
      if (new URL(request.url()).pathname === '/v1/analytics/events:batch') batches.push(request.url());
      if (!['GET', 'HEAD'].includes(request.method())) writes.push(request.method() + ' ' + request.url());
    });
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(() => document.addEventListener('securitypolicyviolation', event => {
      console.error('CSP violation: ' + event.effectiveDirective);
    }));
    // Exercise the strict-region campaign-link handoff without granting consent.
    await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
    const response = await page.goto(fixture.path + query);
    expect(response?.status()).toBe(200);
    expect(response?.headers()['content-security-policy']).toMatch(/script-src [^;]*'nonce-[^']+'/);
    await expect(page.getByRole('button', { name: 'Reject optional', exact: true })).toBeEnabled();

    const bridge = page.getByRole('region', { name: 'Try a file of your own' });
    await expect(bridge.getByRole('link', { name: 'Open Document Studio', exact: true })).toHaveAttribute('href', '/studio?' + campaign);
    await expect(bridge.getByRole('link', { name: 'Anymd Pro — $29 once', exact: true })).toHaveAttribute('href', '/pricing?entry_surface=' + fixture.surface + '&' + campaign);
    const samples = bridge.getByRole('navigation', { name: 'Try a sample' });
    await expect(samples.getByRole('link')).toHaveCount(3);
    await expect(bridge.locator('[style]')).toHaveCount(0);
    for (const [name, href] of links) {
      const link = samples.getByRole('link', { name, exact: true });
      await expect(link).toHaveAttribute('href', href);
      await expectTapTarget(link);
    }

    const pro = page.getByRole('region', { name: 'Verify what your agent cites' });
    if (fixture.path === '/vs/markdownify-mcp') {
      const primary = pro.getByRole('link', { name: 'See Anymd Pro', exact: true });
      const secondary = pro.getByRole('link', { name: fixture.proof, exact: true });
      await expect(primary).toHaveClass(/button-primary/);
      await expect(primary).toHaveAttribute('href', '/pricing?entry_surface=' + fixture.surface + '&' + campaign);
      await expect(secondary).toHaveAttribute('href', citation);
      const first = await expectTapTarget(primary);
      const second = await expectTapTarget(secondary);
      if (first.y === second.y) expect(second.x).toBeGreaterThan(first.x + first.width);
      else expect(second.y).toBeGreaterThan(first.y + first.height);
      await expect(pro.locator('[style]')).toHaveCount(0);
    }
    await expectNoOverflow(page);

    if (fixture.proof) {
      const source = fixture.path === '/vs/markdownify-mcp' ? pro : samples;
      const link = source.getByRole('link', { name: fixture.proof, exact: true });
      if (hasTouch) await link.tap();
      else await link.click();
      await expect(page).toHaveURL(citation);
      const details = page.locator('.cite-preview-details');
      await expect(details).toHaveJSProperty('open', true);
      await expect(details.getByText('Exact quote verified', { exact: true })).toBeVisible();
      await expect(details.locator('figcaption')).toContainText('sample.pdf');
      await expect(details.locator('svg rect')).toBeVisible();
      await expect(page.locator('.studio-label')).toContainText('sample.pdf');
      await expect(page.locator('.studio-code')).toContainText('Quarterly Field Report', { timeout: 15_000 });
      await expect(page.locator(fragment)).toBeInViewport();
      await expectNoOverflow(page);
    }
    expect(await page.evaluate(key => localStorage.getItem(key), signalChoiceKey)).toBeNull();
    expect(batches, 'pre-consent Signal batches').toEqual([]);
    expect(writes, 'no document uploads or other writes').toEqual([]);
    expect(errors, 'browser console, runtime and CSP errors').toEqual([]);
  });
}

test('the sample nav is absent from legal, security and noindex pages', async ({ page }) => {
  for (const path of ['/privacy', '/legal/terms', '/security/remote-url-policy', '/docs/security', '/docs/pro-licence-testing']) {
    expect((await page.goto(path))?.status(), path).toBe(200);
    await expect(page.getByRole('region', { name: 'Try a file of your own' })).toHaveCount(0);
    await expect(page.getByRole('navigation', { name: 'Try a sample' })).toHaveCount(0);
  }
});
