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

test.describe('Python installation without JavaScript', () => {
  test.use({ javaScriptEnabled: false });

  const pypi = 'https://pypi.org/project/anymd/';
  for (const route of ['/', '/vs/llamaparse', '/vs/docling', '/docs/python']) {
    test(`shows PyPI install options on ${route}`, async ({ page }) => {
      expect((await page.goto(route))?.status()).toBe(200);
      const content = route === '/'
        ? page.getByRole('group', { name: 'Installation options' })
        : page.getByRole('main');
      await expect(content.locator('code').filter({ hasText: 'pip install anymd' }).first()).toBeVisible();
      await expect(content.locator('code').filter({ hasText: 'uvx anymd' }).first()).toBeVisible();
      await expect(content.locator(`a[href="${pypi}"]`).first()).toBeVisible();
      await expect(content).not.toContainText(/not yet published to PyPI|not on PyPI yet/i);
      if (route === '/') await expect(content.getByText('npx -y @sylphx/anymd setup')).toBeVisible();
    });
  }

  test('lists the Python install options in llms.txt', async ({ request }) => {
    const response = await request.get('/llms.txt');
    expect(response.status()).toBe(200);
    expect(await response.text()).toContain('Python (PyPI): `pip install anymd` or `uvx anymd` (' + pypi + ')');
  });
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

test('converts a dropped document without sending it to the server', async ({ page }) => {
  await page.goto('/studio');
  const dataTransfer = await page.evaluateHandle(() => {
    const transfer = new DataTransfer();
    transfer.items.add(new File(['<h1>Local drop fixture</h1>'], 'local.html', { type: 'text/html' }));
    return transfer;
  });
  const writes: string[] = [];
  page.context().on('request', (request) => {
    if (!['GET', 'HEAD'].includes(request.method())) writes.push(request.url());
  });
  await page.getByText('Drop a document here', { exact: true }).dispatchEvent('drop', { dataTransfer });
  await expect(page.getByLabel('Markdown output')).toContainText('# Local drop fixture');
  expect(writes).toEqual([]);
});

test('CLI consent page strips the query without suppressing the form origin', async ({ page }) => {
  await page.goto('/cli-auth');
  await expect(page.locator('meta[name="referrer"]')).toHaveAttribute('content', 'strict-origin');
});

test('shows one-time 16-currency pricing', async ({ page }) => {
  await page.goto('/pricing');
  await expect(page.getByText('$29', { exact: true })).toBeVisible();
  const currency = page.getByLabel('Currency');
  await expect(currency.locator('option')).toHaveCount(16);
  await currency.selectOption('HKD');
  await expect(page.getByText('HK$229', { exact: true })).toBeVisible();
});

test('pricing and its Pro alias put the price above the four concrete benefits', async ({ page }) => {
  for (const path of ['/pricing', '/pro']) {
    await page.goto(path);
    const plan = page.getByRole('article').filter({ has: page.getByRole('heading', { name: 'Anymd Pro', exact: true }) });
    const benefits = plan.getByRole('listitem');
    await expect(benefits).toHaveText([
      'Everything in Free OSS',
      'PDF cite-check with exact page & bounding-box proof',
      'Hashed video-frame timeline extraction & citations',
      'Works offline after activation on each machine',
    ]);
    await expect(plan.locator('li svg[aria-hidden="true"]')).toHaveCount(4);
    expect(await plan.getByText('$29', { exact: true }).evaluate((price) => price.getBoundingClientRect().bottom)).toBeLessThanOrEqual(
      await benefits.first().evaluate((first) => first.getBoundingClientRect().top),
    );
  }
});

test('header sends signed-out users to the provider chooser', async ({ page }) => {
  await page.route('**/api/auth/session*', (route) => route.fulfill({ json: { authConfigured: true, user: null } }));
  const session = page.waitForResponse('**/api/auth/session');
  await page.goto('/');
  await session;
  const header = page.getByRole('banner').getByRole('link', { name: 'Sign in', exact: true });
  await expect(header).toBeVisible();
  await expect(header).toHaveAttribute('href', '/sign-in?next=%2Fdashboard');
  await header.click();
  await expect(page).toHaveURL('/sign-in?next=%2Fdashboard');
  const chooser = page.getByRole('main');
  await expect(chooser.getByRole('link', { name: 'Continue with Google' })).toHaveAttribute('href', '/api/auth/google?next=%2Fdashboard');
});
