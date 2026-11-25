import { expect, test } from '@playwright/test';

const origin = 'https://anymd.sylphx.com';
const comparison = '/vs/markdownify-mcp';

for (const route of ['/', '/pricing', comparison]) {
  test(`${route} exposes canonical metadata and structured data matching visible copy`, async ({ page, request }) => {
    const url = origin + route;
    const response = await request.get(route);
    expect(response.ok()).toBe(true);
    const html = await response.text();
    expect(html).toContain('"@type":"SoftwareApplication"');
    expect(html).toContain('"@type":"FAQPage"');
    await page.goto(route);
    await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', url);
    await expect(page.locator('meta[property="og:url"]')).toHaveAttribute('content', url);
    await expect(page.locator('meta[name="twitter:card"]')).toHaveAttribute('content', 'summary_large_image');
    const graph = JSON.parse(await page.locator('script[type="application/ld+json"]').textContent() ?? '')['@graph'];
    const [software, faq] = graph;
    expect(software).toMatchObject({ '@type': 'SoftwareApplication', name: 'Anymd' });
    expect(software.offers.map((offer: {price: number}) => offer.price)).toEqual(route === comparison ? [0] : [0, 29]);
    expect(software.offers.every((offer: {'@type': string; priceCurrency: string}) => offer['@type'] === 'Offer' && offer.priceCurrency === 'USD')).toBe(true);
    expect(faq).toMatchObject({ '@type': 'FAQPage' });
    const entries = faq.mainEntity.map(({ name, acceptedAnswer }: { name: string; acceptedAnswer: { text: string } }) => [name, acceptedAnswer.text]).flat();
    await expect(page.locator('.faq-list').locator('dt, dd')).toHaveText(entries);
  });
}

test('comparison is discoverable and its local setup controls work', async ({ page }) => {
  for (const path of ['/vs', '/docs']) {
    await page.goto(path);
    await expect(page.getByRole('main').locator(`a[href="${comparison}"]`)).toBeVisible();
  }
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', {value: {writeText: async (text: string) => sessionStorage.setItem('test.copied', text)}});
    localStorage.setItem('anymd.cookie-settings.v1', 'reject');
  });
  await page.goto(comparison);
  await expect(page).toHaveTitle(/Markdownify MCP alternative/);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Anymd vs Markdownify MCP');
  await expect(page.getByRole('table')).toBeVisible();
  const width = page.viewportSize()!.width;
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
  await expect(page.getByRole('link', { name: 'Try a file locally' })).toHaveAttribute('href', '/studio');
  await page.getByRole('link', { name: 'Get the CLI and MCP server' }).click();
  await expect(page).toHaveURL(comparison + '#install');
  await page.getByRole('button', { name: 'Copy MCP configuration' }).click();
  await expect(page.getByRole('button', { name: 'Copied', exact: true })).toBeVisible();
  const mcp = await page.evaluate(() => JSON.parse(sessionStorage.getItem('test.copied') ?? ''));
  expect(mcp).toEqual({ mcpServers: { anymd: { command: 'npx', args: ['-y', '@sylphx/anymd', 'mcp', '--allow-dir=/path/to/docs'] } } });
  await expect(page.getByRole('contentinfo').getByRole('link', { name: 'Comparisons' })).toHaveAttribute('href', '/vs');
});

test('homepage leads with free Studio and a priced Pro alternative; sign-in is not indexable', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('link', { name: 'Try Studio Free' })).toHaveAttribute('href', '/studio');
  await expect(page.getByRole('link', { name: 'Get Anymd Pro — $29' })).toHaveAttribute('href', '/pricing?entry_surface=home_hero');
  await expect(page.getByRole('main').getByRole('link', { name: 'Sign in', exact: true })).toHaveCount(0);
  await page.goto('/sign-in');
  await expect(page.locator('meta[name="robots"]')).toHaveAttribute('content', 'noindex');
});
