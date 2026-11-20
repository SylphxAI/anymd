import { expect, test } from '@playwright/test';

const routes = ['/', '/docs', '/docs/cli', '/guide/getting-started', '/vs/docling', '/pricing', '/pro', '/pro/thanks', '/recover', '/account', '/billing', '/dashboard', '/studio', '/playground'];

test('public and account entry points render without horizontal overflow', async ({ page }) => {
  for (const route of routes) {
    expect((await page.goto(route))?.status(), route).toBe(200);
    await expect(page.locator('h1'), route).toBeVisible();
    await expect(page.locator('.header-actions')).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth), route).toBeLessThanOrEqual(page.viewportSize()!.width);
  }
  const index = await page.request.get('/llms.txt');
  expect(await index.text()).toContain('anymd Pro');
});

test('converts a local document and locates an exact source quote', async ({ page }) => {
  await page.goto('/studio');
  await page.getByRole('button', { name: 'Sample document' }).click();
  const output = page.getByLabel('Markdown output');
  await expect(output).toContainText('Field Notes', { timeout: 15_000 });
  const input = page.getByLabel('Quote from the document');
  await input.fill('three teams sampled');
  await page.getByRole('button', { name: 'Find quote' }).click();
  await expect(page.getByText(/Found in/i)).toBeVisible();
  await input.fill('a phrase not found in the source');
  await page.getByRole('button', { name: 'Find quote' }).click();
  await expect(page.getByText(/No exact match/i)).toBeVisible();
});

test('CLI consent page strips the query without suppressing the form origin', async ({ page }) => {
  await page.goto('/cli-auth');
  await expect(page.locator('meta[name="referrer"]')).toHaveAttribute('content', 'strict-origin');
});

test('shows the provider choices and one-time 16-currency pricing', async ({ page }) => {
  await page.goto('/pricing');
  await expect(page.getByText('$29', { exact: true })).toBeVisible();
  const currency = page.getByLabel('Currency');
  await expect(currency.locator('option')).toHaveCount(16);
  await currency.selectOption('HKD');
  await expect(page.getByText('HK$229', { exact: true })).toBeVisible();
  await page.goto('/sign-in?next=%2Fdashboard');
  await expect(page.getByRole('main').getByText('Continue with Google', { exact: true })).toBeVisible();
  await expect(page.getByText('Continue with GitHub', { exact: true })).toBeVisible();
});
