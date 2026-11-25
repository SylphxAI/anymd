import { expect, test, type Page } from '@playwright/test';
import { fixtureLegacySession } from './origins';

const touch = 'gclid=TEST999&utm_source=google&utm_medium=cpc';
const comparison = '/vs/markdownify-mcp';
const pro = '/pricing?entry_surface=seo_vs-markdownify-mcp';
const banner = (page: Page) => page.getByRole('complementary', {name:'Cookie settings'});
const queue = (page: Page) => page.evaluate(() => ((Reflect.get(window, 'dataLayer') ?? []) as ArrayLike<unknown>[]).map(entry => Array.from(entry)));

async function isolate(page: Page) {
  const hits: string[] = [];
  await page.route('**/cdn-cgi/trace', route => route.fulfill({body:'loc=DE\n'}));
  await page.route(url => /(?:^|\.)(?:googletagmanager|google-analytics|googleadservices|google|doubleclick)\.(?:com|net)$/.test(url.hostname), route => {
    hits.push(route.request().url());
    return route.fulfill({contentType:'application/javascript', body:''});
  });
  await page.route(url => url.pathname === '/v1/analytics/events:batch', route => {
    hits.push(route.request().url());
    return route.fulfill({status:204});
  });
  return hits;
}

test('comparison pre-consent links carry only allowed campaign values into pricing', async ({ page }) => {
  const hits = await isolate(page);
  const response = await page.goto(comparison + '?' + touch + '&entry_surface=cli_pdf&email=private');
  const article = page.locator('article.comparison-page');
  const cta = article.getByRole('link', {name:'See Anymd Pro', exact:true});
  expect(await response!.text()).toContain('href="' + (pro + '&' + touch).replaceAll('&', '&amp;') + '"');
  await expect(banner(page).getByRole('button', {name:'Reject optional'})).toBeEnabled();
  for (const path of ['/vs', '/studio', '/docs/benchmarks', '/docs', '/docs/tools', '/docs/formats']) {
    await expect(article.locator('a[href="' + path + '?' + touch + '"]')).toBeVisible();
  }
  await expect(cta).toHaveAttribute('href', pro + '&' + touch);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
  await cta.click();
  await expect(page).toHaveURL(pro + '&' + touch);
  await expect(page.getByRole('link', {name:'Sign in to buy Pro'})).toBeVisible();
  expect(await queue(page)).toEqual([]);
  expect(hits).toEqual([]);
});

test('comparison drops campaign values when rejected and emits no copy or CTA telemetry', async ({ page, context }) => {
  const hits = await isolate(page);
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto(comparison + '?' + touch);
  const cta = page.getByRole('link', {name:'See Anymd Pro', exact:true});
  await expect(cta).toHaveAttribute('href', pro + '&' + touch);
  await banner(page).getByRole('button', {name:'Reject optional'}).click();
  await expect(cta).toHaveAttribute('href', pro);
  await expect(page.locator('article a[href*="gclid="]')).toHaveCount(0);
  await page.getByRole('button', {name:'Copy command', exact:true}).click();
  await expect(page.getByRole('button', {name:'Copied', exact:true})).toBeVisible();
  await cta.click();
  await expect(page).toHaveURL(pro);
  expect(await queue(page)).toEqual([]);
  expect(hits).toEqual([]);
});

test('comparison copies and Pro CTA send only allowlisted analytics after opt-in', async ({ page, context, baseURL }) => {
  const hits = await isolate(page);
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto(comparison + '?' + touch + '&email=private#private');
  const buttons = ['Copy command', 'Copy CLI commands', 'Copy MCP configuration'];
  const copy = (label: string) => page.getByRole('button', {name:label, exact:true});
  await expect(banner(page).getByRole('button', {name:'Analytics only'})).toBeEnabled();
  await copy(buttons[0]).click();
  await expect(copy('Copied')).toBeVisible();
  expect(await queue(page)).toEqual([]);
  expect(hits).toEqual([]);
  await banner(page).getByRole('button', {name:'Analytics only'}).click();
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  await expect(copy(buttons[0])).toBeVisible();
  const details = {page_location:baseURL + '/vs', page_referrer:'', page_title:'Anymd', send_to:'G-R4TD2Z8RR7'};
  for (const [index, label] of buttons.entries()) {
    const code = await copy(label).locator('..').locator('pre code').textContent();
    await copy(label).click();
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(code);
    const kinds = ['convert_command', 'cli_install', 'mcp_config'].slice(0, index + 1);
    await expect.poll(async () => (await queue(page)).filter(row => row[1] === 'output_copied')).toEqual(kinds.map(command_kind => [
      'event', 'output_copied', {...details, surface:'markdownify_mcp', command_kind},
    ]));
  }
  await page.getByRole('link', {name:'See Anymd Pro', exact:true}).click();
  await expect(page).toHaveURL(pro);
  await expect.poll(async () => (await queue(page)).filter(row => row[1] === 'pro_cta_clicked')).toEqual([
    ['event', 'pro_cta_clicked', {...details, entry_surface:'seo_vs-markdownify-mcp'}],
  ]);
});

test('task index, docs sidebar and docs search preserve campaign in both hrefs and navigation', async ({ page }) => {
  const hits = await isolate(page);
  await page.goto('/tasks?' + touch);
  const guide = page.getByRole('link', {name:/Check a PDF citation/});
  await expect(banner(page).getByRole('button', {name:'Reject optional'})).toBeEnabled();
  await expect(guide).toHaveAttribute('href', '/tasks/check-pdf-citation?' + touch);
  await expect(page.getByRole('link', {name:'Back to documentation'})).toHaveAttribute('href', '/docs?' + touch);
  await guide.click();
  await expect(page).toHaveURL('/tasks/check-pdf-citation?' + touch);
  const nav = page.getByRole('navigation', {name:'Documentation'});
  await nav.getByRole('link', {name:'Getting started'}).click();
  await expect(page).toHaveURL('/docs?' + touch);
  await expect(nav.getByRole('link', {name:'Python API'})).toHaveAttribute('href', '/docs/python?' + touch);
  await page.getByRole('searchbox', {name:'Search documentation'}).fill('python');
  await page.locator('.docs-search').locator('a[href="/docs/python?' + touch + '"]').click();
  await expect(page).toHaveURL('/docs/python?' + touch);
  await expect(page.locator('article.prose h1')).toContainText('Python');
  expect(hits).toEqual([]);
});

test('signed-in free dashboard upgrade and CLI docs retain campaign until rejection', async ({ page, context, baseURL }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires loopback Auth fixture');
  const hits = await isolate(page);
  await context.addCookies([{name:'anymd_session', value:fixtureLegacySession, url:baseURL!}]);
  // Use the loopback Auth session and stub only the read-only plan result.
  await page.route('**/api/anymd/account', route => route.fulfill({json:{plan:'Free OSS'}}));
  await page.goto('/dashboard?' + touch);
  await expect(banner(page).getByRole('button', {name:'Reject optional'})).toBeEnabled();
  const upgrade = page.getByRole('link', {name:'Upgrade to Anymd Pro'});
  const install = page.getByRole('link', {name:'Install the CLI', exact:true});
  await expect(upgrade).toHaveAttribute('href', '/pricing?' + touch);
  await expect(install).toHaveAttribute('href', '/docs?' + touch);
  await banner(page).getByRole('button', {name:'Reject optional'}).click();
  await expect(upgrade).toHaveAttribute('href', '/pricing');
  await expect(install).toHaveAttribute('href', '/docs');
  expect(hits).toEqual([]);
});
