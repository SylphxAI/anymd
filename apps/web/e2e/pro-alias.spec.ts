import { expect, test } from '@playwright/test';

test('/pro redirects to /pricing with one allowlisted entry surface', async ({ request }) => {
  for (const [from, to] of [
    ['/pro', '/pricing?entry_surface=pro'],
    ['/pro/', '/pricing?entry_surface=pro'],
    ['/pro?currency=HKD&utm_source=docs+nav&gclid=Ab%2bc', '/pricing?currency=HKD&utm_source=docs+nav&gclid=Ab%2bc&entry_surface=pro'],
    ['/pro?entry_surface=cli_pdf&currency=EUR', '/pricing?entry_surface=cli_pdf&currency=EUR'],
    ['/pro?entry_surface=private@example.invalid&tag=one&tag=two', '/pricing?tag=one&tag=two&entry_surface=pro'],
    ['/pro?entry_surface=seo_vs-marker&entry_surface=npm_citra', '/pricing?entry_surface=pro'],
  ]) {
    for (const method of ['GET', 'HEAD']) {
      const response = await request.fetch(from, { method, maxRedirects: 0 });
      expect(response.status(), method + ' ' + from).toBe(308);
      expect(response.headers().location, method + ' ' + from).toBe(to);
    }
    const landing = await request.get(to, { maxRedirects: 0 });
    expect(landing.status(), to).toBe(200);
    expect(landing.headers().location, to).toBeUndefined();
  }
  const thanks = await request.get('/pro/thanks?session_id=fixture', { maxRedirects: 0 });
  expect(thanks.status()).toBe(200);
  expect(thanks.headers().location).toBeUndefined();
  const post = await request.post('/pro', { maxRedirects: 0 });
  expect(post.headers().location).toBeUndefined();
});

test('/pro keeps the fragment and lands on the Pricing page', async ({ page }) => {
  await page.goto('/pro?currency=EUR#plans');
  await expect(page).toHaveURL('/pricing?currency=EUR&entry_surface=pro#plans');
  await expect(page.getByRole('heading', { name: 'Anymd Pro', exact: true })).toBeVisible();
  await expect(page.getByLabel('Currency')).toHaveValue('EUR');
});
