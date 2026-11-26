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

for (const [home, pricing, signInPath, signIn] of [
  ['/', '/pricing', '/sign-in', 'Sign in to buy Pro'],
  ['/zh-TW', '/zh-TW/pricing', '/zh-TW/sign-in', '登入以購買 Pro'],
]) {
  test(home + ' Pro links preserve their own entry surface and campaign through pricing and sign-in', async ({ page }) => {
    const hits = await isolate(page);
    for (const [selector, label, surface] of [
      ['.hero-actions', /Anymd Pro/, 'home_hero'],
      ['.pro-panel', /Pro/, 'home_pro_panel'],
    ] as const) {
      await test.step(surface, async () => {
        const response = await page.goto(home + '?' + touch + '&entry_surface=cli_pdf');
        const checkout = pricing + '?entry_surface=' + surface;
        const href = checkout + '&' + touch;
        expect(await response!.text()).toContain('href="' + href.replaceAll('&', '&amp;') + '"');
        const cta = page.locator(selector).getByRole('link', {name:label});
        await expect(cta).toBeVisible();
        await expect(cta).toHaveAttribute('href', href);
        await cta.click();
        await expect(page).toHaveURL(href);
        await expect(page.getByRole('link', {name:signIn, exact:true})).toHaveAttribute(
          'href', signInPath + '?next=' + encodeURIComponent(checkout) + '&' + touch,
        );
      });
    }
    expect(hits).toEqual([]);
  });
}

test('comparison pre-consent links carry only allowed campaign values into pricing', async ({ page }) => {
  const hits = await isolate(page);
  const response = await page.goto(comparison + '?' + touch + '&entry_surface=cli_pdf&email=private');
  const article = page.locator('article.comparison-page');
  const cta = article.getByRole('link', {name:'See Anymd Pro', exact:true});
  expect(await response!.text()).toContain('href="' + (pro + '&' + touch).replaceAll('&', '&amp;') + '"');
  await expect(banner(page).getByRole('button', {name:'Reject optional'})).toBeEnabled();
  for (const path of ['/vs', '/docs/benchmarks', '/docs', '/docs/tools', '/docs/formats']) {
    await expect(article.locator('a[href="' + path + '?' + touch + '"]')).toBeVisible();
  }
  await expect(article.getByRole('link', {name:'Open the PDF sample'})).toHaveAttribute('href', '/studio?sample=pdf&' + touch);
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
  const commands = [
    ['One-shot CLI', 'Copy command', 'convert_command'],
    ['Global CLI', 'Copy CLI commands', 'cli_install'],
    ['MCP JSON', 'Copy MCP configuration', 'mcp_config'],
    ['Python uvx', 'Copy uvx command', 'uvx'],
  ];
  const setup = page.locator('#install');
  const copy = (label: string) => setup.getByRole('button', {name:label, exact:true});
  await expect(banner(page).getByRole('button', {name:'Analytics only'})).toBeEnabled();
  await copy('Copy command').click();
  await expect(copy('Copied')).toBeVisible();
  expect(await queue(page)).toEqual([]);
  expect(hits).toEqual([]);
  await banner(page).getByRole('button', {name:'Analytics only'}).click();
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  await expect(copy('Copy command')).toBeVisible();
  const details = {page_location:baseURL + '/vs', page_referrer:'', page_title:'Anymd', send_to:'G-R4TD2Z8RR7'};
  const copies = async () => (await queue(page)).filter(row => row[1] === 'output_copied');
  const commandEvents = commands.map(([, , command_kind]) => [
    'event', 'output_copied', {...details, surface:'markdownify_mcp', command_kind},
  ]);
  for (const [index, [tab, label]] of commands.entries()) {
    await setup.getByRole('tab', {name:tab, exact:true}).click();
    await copy(label).click();
    await expect.poll(copies).toEqual(commandEvents.slice(0, index + 1));
  }
  await page.getByRole('button', {name:'Copy Markdown', exact:true}).click();
  await expect.poll(copies).toEqual([...commandEvents, ['event', 'output_copied', {...details, surface:'markdownify_mcp_demo'}]]);
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
