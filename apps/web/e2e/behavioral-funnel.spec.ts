import { expect, type Page } from '@playwright/test';
import { signalChoiceKey } from '../src/lib/consent';
import { test } from './fixture-test';

const privacyMode = 'globalPrivacyControl';
const readQueue = (page: Page) => page.evaluate(() => {
  const queue = Reflect.get(window, 'dataLayer') as ArrayLike<unknown>[] | undefined;
  return (queue ?? []).map(entry => Array.from(entry));
});
const events = async (page: Page, name: string) => (await readQueue(page)).filter(row => row[0] === 'event' && row[1] === name);

async function isolate(page: Page, region = 'DE') {
  const hits: string[] = [];
  await page.route(url => /(?:^|\.)(?:googletagmanager|google-analytics|googleadservices|google|doubleclick)\.(?:com|net)$/.test(url.hostname), route => {
    hits.push(route.request().url());
    return route.fulfill({contentType:'application/javascript', body:''});
  });
  await page.route('**/cdn-cgi/trace', route => route.fulfill({body:`loc=${region}\n`}));
  return hits;
}

async function expectEvent(page: Page, name: string, pathname: string, payloads: object[]) {
  const origin = new URL(page.url()).origin;
  await expect.poll(() => events(page, name)).toEqual(payloads.map(payload => ['event', name, {
    ...payload, page_location:origin + pathname, page_title:'Anymd', page_referrer:'', send_to:'G-R4TD2Z8RR7',
  }]));
}

for (const locale of ['zh-TW', 'zh-Hant']) {
  test(`${locale} pricing emits one canonical pricing view after opt-in`, async ({ page }) => {
    const hits = await isolate(page);
    await page.goto(`/${locale}?proof=private-home#private`);
    const optIn = page.getByRole('button', {name:'僅限分析', exact:true});
    await expect(optIn).toBeEnabled();
    expect(await readQueue(page)).toEqual([]);
    expect(hits).toEqual([]);
    await optIn.click();
    await expectEvent(page, 'page_view', '/zh-TW', [{}]);
    if (locale === 'zh-TW') await page.getByRole('link', {name:/選購 Anymd Pro/}).click();
    else await page.goto('/zh-Hant/pricing?proof=private-pricing#private');
    await expectEvent(page, 'pricing_view', '/zh-TW/pricing', [{}]);
    expect(JSON.stringify(await readQueue(page))).not.toMatch(/private-(?:home|pricing)|#private/);
  });
}

for (const mode of ['no choice', 'denied', 'browser GPC', 'HTTP GPC'] as const) {
  test(`${mode} blocks behavioral events; denied/GPC also block localized pricing`, async ({ page }) => {
    const hits = await isolate(page, mode === 'no choice' ? 'US' : 'DE');
    if (mode === 'HTTP GPC') await page.setExtraHTTPHeaders({'Sec-GPC':'1'});
    await page.addInitScript(({key, gpc, mode}) => {
      if (mode === 'browser GPC') Object.defineProperty(navigator, gpc, {value:true});
      if (mode !== 'no choice') localStorage.setItem(key, mode === 'denied' ? 'denied' : 'granted');
    }, {key:signalChoiceKey, gpc:privacyMode, mode});
    await page.goto('/zh-Hant/pricing?proof=private-price');
    if (mode === 'no choice') await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
    else {
      // Open settings to wait for the active consent provider before asserting silence.
      await page.getByRole('button', {name:'Cookie 設定', exact:true}).click();
      await expect(page.getByRole('button', {name:'拒絕選用項目'})).toBeEnabled();
      expect(await readQueue(page)).toEqual([]);
    }
    await page.goto('/studio');
    await page.getByRole('button', {name:'Sample PDF'}).click();
    await expect(page.getByLabel('Markdown output')).toContainText('Quarterly Field Report');
    await page.getByText('See the verified quote in sample.pdf').click();
    await expect(page.getByText('Exact quote verified')).toBeVisible();
    for (const name of ['document_converted', 'paywall_view']) expect(await events(page, name)).toEqual([]);
    if (mode !== 'no choice') {
      expect(await readQueue(page)).toEqual([]);
      expect(hits).toEqual([]);
    }
  });
}

test('Studio reports only format, counts and predefined labels for real WASM and export actions', async ({ page }) => {
  await isolate(page);
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto('/studio?filename=private-filename#private-fragment');
  await page.getByRole('button', {name:'Analytics only', exact:true}).click();
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  await page.getByRole('button', {name:'Sample PDF'}).click();
  const output = page.getByLabel('Markdown output');
  await expect(output).toContainText('Quarterly Field Report');
  const conversion = {file_format:'pdf', source_kind:'sample'};
  await expectEvent(page, 'document_converted', '/studio', [{...conversion, unit_count:2, token_count:expect.any(Number), duration_ms:expect.any(Number)}]);
  await page.getByRole('button', {name:'Copy Markdown', exact:true}).click();
  await expect(page.getByRole('button', {name:'Copied', exact:true})).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(await output.textContent());
  await expectEvent(page, 'output_copied', '/studio', [{surface:'studio_output'}]);
  const download = page.waitForEvent('download');
  await page.getByRole('button', {name:'Download .md', exact:true}).click();
  expect((await download).suggestedFilename()).toBe('sample.md');
  await expectEvent(page, 'output_downloaded', '/studio', [conversion]);
  const quote = page.getByLabel('Quote from the document');
  await quote.fill('private quote sentinel');
  await page.getByRole('button', {name:'Find quote'}).click();
  await quote.fill('Quarterly Field Report');
  await page.getByRole('button', {name:'Find quote'}).click();
  await expectEvent(page, 'quote_searched', '/studio', [{matched:false}, {matched:true}]);
  const toggle = page.getByText('See the verified quote in sample.pdf');
  await toggle.click();
  await expectEvent(page, 'paywall_view', '/studio', [{entry_surface:'studio_cite_preview'}]);
  await toggle.click();
  await expect(page.getByText('Exact quote verified')).toBeHidden();
  await page.locator('input[type=file]').setInputFiles({name:'private-local.txt', mimeType:'text/plain', buffer:Buffer.from('do not send private@example.invalid')});
  await expect(output).toContainText('private@example.invalid');
  await expectEvent(page, 'document_converted', '/studio', [
    {...conversion, unit_count:2, token_count:expect.any(Number), duration_ms:expect.any(Number)},
    {file_format:'text', source_kind:'local_file', unit_count:1, token_count:expect.any(Number), duration_ms:expect.any(Number)},
  ]);
  await page.getByRole('button', {name:'Sample video'}).click();
  await expect.poll(() => page.locator('video').evaluate((video: HTMLVideoElement) => video.readyState)).toBeGreaterThan(0);
  await page.getByRole('button', {name:'Mark current timestamp'}).click();
  await page.getByRole('button', {name:'Copy timestamp'}).click();
  await expectEvent(page, 'output_copied', '/studio', [{surface:'studio_output'}, {surface:'studio_timestamp'}]);
  await page.getByRole('link', {name:/Get Anymd Pro/}).click();
  await expect(page).toHaveURL('/pricing?entry_surface=studio_cite_preview');
  await expectEvent(page, 'pro_cta_clicked', '/studio', [{entry_surface:'studio_cite_preview'}]);
  await expectEvent(page, 'paywall_view', '/studio', [{entry_surface:'studio_cite_preview'}]);
  expect(JSON.stringify(await readQueue(page))).not.toMatch(/private|Quarterly Field Report|sample\.pdf|demo\.mp4/);
});

test('home and reference copies and home Pro clicks emit predefined labels', async ({ page }) => {
  await isolate(page);
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.addInitScript(key => localStorage.setItem(key, 'analytics_only'), signalChoiceKey);
  await page.goto('/');
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  await page.getByRole('button', {name:'Copy install command', exact:true}).click();
  await page.getByRole('button', {name:'Copy Markdown', exact:true}).click();
  await expectEvent(page, 'output_copied', '/', [{surface:'home_install', command_kind:'npm'}, {surface:'home_demo'}]);
  await page.locator('.hero-actions').getByRole('link', {name:/Anymd Pro/}).click();
  await expectEvent(page, 'pro_cta_clicked', '/', [{entry_surface:'home_hero'}]);
  // The click event can land before the client-side route commits to history.
  await expect(page).toHaveURL('/pricing?entry_surface=home_hero');
  await page.goBack();
  await expect(page).toHaveURL('/');
  await page.locator('.pro-panel').getByRole('link').click();
  await expectEvent(page, 'pro_cta_clicked', '/', [{entry_surface:'home_hero'}, {entry_surface:'home_pro_panel'}]);
  await page.goto('/docs');
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  await page.getByRole('button', {name:'Copy code', exact:true}).first().click();
  await expectEvent(page, 'output_copied', '/docs', [{surface:'docs_code'}]);
});

test('generic documentation reports the article_bridge Pro event after opt-in', async ({ page }) => {
  await isolate(page);
  await page.goto('/docs?utm_source=docs&proof=private-doc');
  await page.getByRole('button', {name:'Analytics only', exact:true}).click();
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  const bridge = page.getByRole('region', {name:'Try a file of your own'});
  await bridge.getByRole('link', {name:'Anymd Pro — $29 once'}).click();
  await expectEvent(page, 'pro_cta_clicked', '/docs', [{entry_surface:'article_bridge'}]);
  await expect(page).toHaveURL(url => url.pathname === '/pricing' && url.searchParams.get('entry_surface') === 'article_bridge');
});

test('task guide copies exact code without page overflow and measures only after opt-in', async ({ page, context }) => {
  const hits = await isolate(page);
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto('/tasks/check-pdf-citation?proof=private-task#private');

  const article = page.locator('article');
  const code = article.locator('pre > code');
  await expect(code.first()).toContainText('python3 make-citation-pdf.py');
  const buttons = article.getByRole('button', {name:'Copy code', exact:true});
  await expect(code).toHaveCount(3);
  await expect(buttons).toHaveCount(3);
  for (const button of await buttons.all()) await expect(button).toBeVisible();
  // The Playwright projects exercise both 390px and 1440px widths.
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);

  const optIn = page.getByRole('button', {name:'Analytics only', exact:true});
  await expect(optIn).toBeEnabled();
  const first = article.locator('.doc-code').first();
  await first.getByRole('button', {name:'Copy code', exact:true}).click();
  await expect(first.getByRole('button', {name:'Copied', exact:true})).toBeVisible();
  expect(await readQueue(page)).toEqual([]);
  expect(hits).toEqual([]);

  await optIn.click();
  await expectEvent(page, 'page_view', '/tasks', [{}]);
  const last = article.locator('.doc-code').last();
  await last.getByRole('button', {name:'Copy code', exact:true}).click();
  await expect(last.getByRole('button', {name:'Copied', exact:true})).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(await code.last().textContent());
  await expectEvent(page, 'output_copied', '/tasks', [{surface:'docs_code'}]);
});

test('signed-in copy and CLI form events exclude account info and all native proofs', async ({ page, context, baseURL }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires loopback Auth fixture');
  await isolate(page);
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.addInitScript(key => localStorage.setItem(key, 'analytics_only'), signalChoiceKey);
  await context.addCookies([{name:'anymd_session', value:'identity_org_session_fixture_legacy', url:baseURL!}]);
  await page.goto('/dashboard');
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  await page.getByRole('button', {name:'Copy MCP config'}).click();
  await page.getByRole('button', {name:'Copy login command'}).click();
  await expectEvent(page, 'output_copied', '/dashboard', [
    {surface:'dashboard_mcp', command_kind:'mcp_config'}, {surface:'dashboard_cli', command_kind:'cli_login'},
  ]);
  const args = new URLSearchParams({port:'32101', state:'a'.repeat(32), challenge:'b'.repeat(64)});
  await page.goto('/cli-auth?' + args);
  await expect(page.getByText('test@example.invalid')).toBeVisible();
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  await page.route('**/api/cli-auth', async route => {
    expect(route.request().method()).toBe('POST');
    expect(new URLSearchParams(route.request().postData() ?? '')).toEqual(args);
    // Keep the consent document alive to inspect its queue before the native redirect.
    await route.fulfill({status:204});
  });
  const submitted = page.waitForResponse('**/api/cli-auth');
  await page.getByRole('button', {name:'Connect this CLI'}).click();
  expect((await submitted).status()).toBe(204);
  await expectEvent(page, 'cli_authorized', '/cli-auth', [{entry_surface:'cli_auth'}]);
  expect(JSON.stringify(await readQueue(page))).not.toMatch(/32101|a{8}|b{8}|test@example\.invalid/);
});
