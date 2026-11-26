import { expect, test, type Page } from '@playwright/test';
import { signalChoiceKey } from '../src/lib/consent';

const fragment = '#studio-cite-heading';
const campaign = 'gclid=fixture&utm_source=pricing&utm_campaign=PDF+Proof';
const locales = [
  { prefix: '', link: 'Inspect citation proof', reject: 'Reject optional', verified: 'Exact quote verified' },
  { prefix: '/zh-TW', link: '查看引文座標', reject: '拒絕選用項目', verified: '引文已逐字核對' },
];
const google = /(?:^|\.)(?:googletagmanager|google-analytics|googleadservices|google|doubleclick)\.(?:com|net)$/;

async function proofIsReady(page: Page, verified: string) {
  const proof = page.locator('.cite-preview-details');
  await expect(proof).toHaveJSProperty('open', true);
  await expect(proof.getByText(verified, { exact: true })).toBeVisible();
  await expect(proof.locator('svg rect')).toBeVisible();
  await expect(page.locator('.studio-label')).toContainText('sample.pdf');
  await expect(page.locator('.studio-code')).toContainText('Quarterly Field Report', { timeout: 15_000 });
}

async function expectCleanLayout(page: Page) {
  await expect(page.locator('main [style]')).toHaveCount(0);
  const { width, viewport } = await page.evaluate(() => ({ width: document.documentElement.scrollWidth, viewport: innerWidth }));
  expect(width).toBeLessThanOrEqual(viewport);
  expect(viewport).toBe(page.viewportSize()?.width);
}

for (const { prefix, link, reject, verified } of locales) {
  test((prefix || 'EN') + ' pricing opens converted PDF and crop proof before consent', async ({ page, context, hasTouch }) => {
    const unexpected: string[] = [];
    context.on('request', request => {
      const url = new URL(request.url());
      if (url.pathname === '/v1/analytics/events:batch' || google.test(url.hostname)) unexpected.push(request.url());
    });
    await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
    await page.addInitScript(() => document.addEventListener('securitypolicyviolation', event => {
      console.error('CSP violation: ' + event.effectiveDirective);
    }));
    page.on('console', message => { if (message.text().startsWith('CSP violation:')) unexpected.push(message.text()); });
    page.on('pageerror', error => unexpected.push(error.message));

    const pricing = (await page.goto(prefix + '/pricing?' + campaign + '&sample=video&unrelated=drop-me'))!;
    expect(pricing.headers()['content-security-policy']).toMatch(/script-src [^;]*'nonce-[^']+'/);
    await expect(page.getByRole('button', { name: reject, exact: true })).toBeEnabled();
    await expectCleanLayout(page);
    const anchor = page.getByRole('link', { name: link, exact: true });
    const destination = prefix + '/studio?sample=pdf&' + campaign + fragment;
    await expect(anchor).toHaveAttribute('href', destination);
    if (hasTouch) await anchor.tap();
    else await anchor.click();
    await expect(page).toHaveURL(destination);
    await proofIsReady(page, verified);
    await expect(page.locator(fragment)).toBeInViewport();
    await expectCleanLayout(page);

    await page.goto(prefix + '/studio' + fragment);
    await proofIsReady(page, verified);
    await expect(page.locator(fragment)).toBeInViewport();
    await expectCleanLayout(page);
    expect(await page.evaluate(key => localStorage.getItem(key), signalChoiceKey)).toBeNull();
    expect(unexpected).toEqual([]);
  });
}

test('opening the proof from a URL is not a paywall view even with analytics consent', async ({ page }) => {
  await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
  await page.route(url => google.test(url.hostname), route => route.fulfill({ contentType: 'application/javascript', body: '' }));
  await page.route('**/v1/analytics/events:batch', route => route.fulfill({ status: 204 }));
  await page.addInitScript(key => localStorage.setItem(key, 'analytics_only'), signalChoiceKey);
  const paywallViews = () => page.evaluate(() => {
    const queue = Reflect.get(window, 'dataLayer') as ArrayLike<unknown>[] | undefined;
    return (queue ?? []).filter(row => row[0] === 'event' && row[1] === 'paywall_view').length;
  });
  await page.goto('/studio?sample=pdf' + fragment);
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  await proofIsReady(page, 'Exact quote verified');
  expect(await paywallViews()).toBe(0);
  const proof = page.locator('.cite-preview-details');
  const summary = proof.locator(':scope > summary');
  await summary.press('Enter');
  await expect(proof).toHaveJSProperty('open', false);
  expect(await paywallViews()).toBe(0);
  await summary.press('Enter');
  await expect(proof).toHaveJSProperty('open', true);
  await expect.poll(paywallViews).toBe(1);
});
