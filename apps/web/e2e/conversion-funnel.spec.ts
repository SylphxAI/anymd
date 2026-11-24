import { expect } from '@playwright/test';
import { test } from './fixture-test';

const seoPaths = [
  '/vs', '/vs/docling', '/vs/kreuzberg', '/vs/llamaparse', '/vs/markdownify-mcp',
  '/vs/marker', '/vs/markitdown', '/vs/mistral-ocr', '/vs/pdftotext', '/vs/pymupdf4llm',
  '/vs/unstructured', '/use-cases/pdf-and-office-to-markdown-mcp',
  '/use-cases/verifiable-video-and-cite-check-ai-agents',
];
const taskPaths = ['/tasks/check-pdf-citation', '/tasks/build-video-timeline', '/tasks/extract-video-frame'];

async function expectNoOverflow(page: import('@playwright/test').Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
}

// Use the sitemap as the crawl surface, and independently require the launch
// pages so removing a broken page from the sitemap cannot make the test pass.
test('public pages have no escaped HTML and SEO pages link directly to pricing', async ({ page, request }) => {
  test.setTimeout(75_000);
  const sitemap = await request.get('/sitemap.xml');
  expect(sitemap.status()).toBe(200);
  const paths = [...(await sitemap.text()).matchAll(/<loc>([^<]+)<\/loc>/g)].map(([, url]) => new URL(url).pathname);
  expect(paths).toEqual(expect.arrayContaining(['/docs', '/tasks', ...seoPaths, ...taskPaths]));

  for (const path of paths) {
    await test.step(path, async () => {
      const response = await page.goto(path, { waitUntil: 'domcontentloaded' });
      expect(response?.status()).toBe(200);
      const main = page.getByRole('main');
      await expect(main.getByRole('heading', { level: 1 })).toBeVisible();
      expect(await main.innerText()).not.toMatch(/<\/?(?:a|p|details|summary)(?:\s|>)/i);
      const article = main.locator('article.prose');
      if (await article.count()) {
        const prose = await article.evaluate(el => {
          const copy = el.cloneNode(true) as HTMLElement;
          copy.querySelectorAll('code, pre').forEach(example => example.remove());
          return copy.textContent;
        });
        expect(prose).not.toMatch(/<(?:!--|\/?[a-z][\w-]*(?=[\s/>]))/i);
      }
      if (path === '/vs/docling') await expect(article.locator('code').getByText('<!-- page N -->', { exact: true }).first()).toBeVisible();
      if (seoPaths.includes(path)) {
        const cta = main.locator('a.button-primary[href^="/pricing?entry_surface=seo_"]').first();
        await expect(cta).toBeVisible();
        await cta.click({ trial: true });
      }
    });
  }
});

test('LlamaParse CTA opens pricing without a legacy redirect or horizontal overflow', async ({ page, request }) => {
  await page.goto('/vs/llamaparse');
  await expectNoOverflow(page);
  const cta = page.getByRole('link', { name: 'See Anymd Pro (US$29 once)', exact: true });
  await expect(cta).toHaveCSS('text-decoration-line', 'none');
  const href = '/pricing?entry_surface=seo_vs-llamaparse';
  await expect(cta).toHaveAttribute('href', href);
  expect((await request.get(href, { maxRedirects: 0 })).status()).toBe(200);
  await cta.click();
  await expect(page).toHaveURL(href);
  await expect(page.getByText('Tax included. No subscription.')).toBeVisible();
});

test('task index is linked from documentation and opens all three guides', async ({ page }) => {
  await page.goto('/docs');
  await page.getByRole('navigation', { name: 'Documentation' }).getByRole('link', { name: 'Task guides' }).click();
  await expect(page).toHaveURL('/tasks');
  await expect(page.getByRole('heading', { name: 'Task guides' })).toBeVisible();
  await expectNoOverflow(page);
  for (const path of taskPaths) {
    await page.getByRole('main').locator('a[href="' + path + '"]').click();
    await expect(page).toHaveURL(path);
    await expect(page.getByRole('main').locator('article h1')).toBeVisible();
    await page.goBack();
  }
});

test('checkout return shows CLI activation without claiming the payment cleared', async ({ page }) => {
  await page.goto('/pro/thanks?session_id=cs_test_Fixture123');
  await expect(page.getByRole('heading', { name: 'Activate Anymd Pro' })).toBeVisible();
  await expect(page.getByText('anymd login', { exact: true })).toBeVisible();
  await expect(page.getByText('After your one-time purchase clears', { exact: false })).toBeVisible();
  await expect(page.getByRole('link', { name: 'check your plan' })).toHaveAttribute('href', '/dashboard');
  await expect(page.getByRole('link', { name: 'Link a legacy purchase' })).toHaveAttribute('href', '/recover?session_id=cs_test_Fixture123');
});

test('old checkout returns still accept a legacy recovery link without retaining its token in the URL', async ({ page }) => {
  await page.goto('/pro/thanks?recovery_token=fixture.fixture');
  await expect(page.getByRole('button', { name: 'Open my recovery link' })).toBeVisible();
  await expect(page).toHaveURL('/pro/thanks');
});

test('signed-in dashboard offers a copyable CLI login command', async ({ page }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires the local Auth fixture');
  await page.goto('/sign-in?next=/dashboard');
  await page.getByRole('link', { name: 'Continue with Google' }).click();
  await expect(page).toHaveURL('/dashboard');
  await expect(page.getByText('anymd login', { exact: true })).toBeVisible();
  await expect(page.getByText(/isn[’']t available yet/i)).toHaveCount(0);
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.getByRole('button', { name: 'Copy login command' }).click();
  await expect(page.getByRole('button', { name: 'Copied', exact: true })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('anymd login');
});
