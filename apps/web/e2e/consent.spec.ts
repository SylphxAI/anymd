import { expect, test, type Page } from '@playwright/test';

const tag = 'https://www.googletagmanager.com/gtag/js?id=G-R4TD2Z8RR7';
const googleHost = /(?:^|\.)(?:googletagmanager\.com|google-analytics\.com|googleadservices\.com|doubleclick\.net|google\.com)$/;
const all = {analytics_storage:'granted', ad_storage:'granted', ad_user_data:'granted', ad_personalization:'denied'};
const analyticsOnly = {...all, ad_storage:'denied', ad_user_data:'denied'};

async function isolate(page: Page, country: string | null, gpc = false) {
  const requests: string[] = [];
  await page.context().route(url => googleHost.test(url.hostname), route => {
    requests.push(route.request().url());
    // No test traffic reaches Google's property. Read the app's actual gtag
    // queue, and simulate its checkout callback so no network is needed.
    return route.fulfill({contentType:'application/javascript', body: 'for (const command of window.dataLayer) if (command[2]?.event_callback) command[2].event_callback();'});
  });
  await page.route('**/cdn-cgi/trace', route => {
    const host = new URL(route.request().url()).hostname;
    return country === null ? route.fulfill({status:503, body:''}) : route.fulfill({body:'h=' + host + '\nloc=' + country + '\n'});
  });
  if (gpc) await page.addInitScript(() => Object.defineProperty(navigator, 'globalPrivacyControl', {value:true}));
  return requests;
}
const panel = (page: Page) => page.getByRole('complementary', {name:'Cookie settings'});
const commands = (page: Page) => page.evaluate(() => ((window as Window & {dataLayer?: ArrayLike<unknown>[]}).dataLayer ?? []).map(command => Array.from(command)));
const events = async (page: Page, name: string) => (await commands(page)).filter(x => x[0] === 'event' && x[1] === name);

for (const [label, code] of [['EEA', 'DE'], ['UK', 'GB'], ['Switzerland', 'CH'], ['unknown', null]] as const) {
  test(label + ' makes no Google requests before choice or after rejection', async ({ page }) => {
    const google = await isolate(page, code);
    await page.goto('/pricing');
    const banner = panel(page);
    await expect(banner).toBeVisible();
    const labels = ['Reject optional', 'Analytics only', 'Allow analytics & ads'];
    const buttons = labels.map(name => banner.getByRole('button', {name, exact:true}));
    for (const button of buttons) {
      await expect(button).toBeVisible();
      await expect(button).toHaveClass('button');
    }
    const rects = await Promise.all(buttons.map(button => button.boundingBox()));
    const width = page.viewportSize()!.width;
    expect(rects.every(box => box && Math.abs(box.width - (rects[0]?.width ?? 0)) < 1 && box.x >= 0 && box.x + box.width <= width)).toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    expect(google).toEqual([]);
    await buttons[0].click();
    await expect(banner).toHaveCount(0);
    const trace = page.waitForResponse('**/cdn-cgi/trace');
    await page.reload();
    await trace;
    await page.getByRole('button', {name:'Cookie settings', exact:true}).click();
    await expect(buttons[0]).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(banner).toHaveCount(0);
    await expect(page.locator('#anymd-google-tag')).toHaveCount(0);
    expect(google).toEqual([]);
  });
}

test('analytics-only opt-in emits clean pricing event; withdrawal stops future Google requests', async ({ page }) => {
  const google = await isolate(page, 'FR');
  await page.goto('/pricing?session_id=cs_test_NOT_FOR_GOOGLE#private');
  expect(google).toEqual([]);
  await panel(page).getByRole('button', {name:'Analytics only'}).click();
  await expect.poll(() => google).toEqual([tag]);
  expect((await commands(page))[0]).toEqual(['consent','default',analyticsOnly]);
  const pricing = await events(page, 'pricing_view');
  expect(pricing).toHaveLength(1);
  expect(JSON.stringify(pricing)).not.toMatch(/cs_test_NOT_FOR_GOOGLE|#private/);
  await page.getByRole('button', {name:'Cookie settings', exact:true}).click();
  const reload = page.waitForNavigation({waitUntil:'networkidle'});
  await panel(page).getByRole('button', {name:'Reject optional'}).click();
  await reload;
  await expect(page.locator('#anymd-google-tag')).toHaveCount(0);
  await page.getByRole('contentinfo').getByRole('link', {name:'Privacy', exact:true}).click();
  await expect(page).toHaveURL('/privacy');
  await expect(page.getByRole('main')).toContainText('Global Privacy Control');
  await expect(page.getByRole('main')).toContainText('22 November 2026');
  expect(google).toEqual([tag]);
});

test('non-EEA starts with full measurement and globally denied personalization', async ({ page }) => {
  const google = await isolate(page, 'US');
  await page.goto('/pricing');
  await expect.poll(() => google).toEqual([tag]);
  await expect(panel(page)).toHaveCount(0);
  expect((await commands(page))[0]).toEqual(['consent','default',all]);
  expect(await events(page, 'pricing_view')).toHaveLength(1);
});

test('GPC overrides the non-EEA defaults and an explicit allow choice', async ({ page }) => {
  const google = await isolate(page, 'US', true);
  const trace = page.waitForResponse('**/cdn-cgi/trace');
  await page.goto('/pricing');
  await trace;
  await expect(panel(page)).toHaveCount(0);
  await page.getByRole('button', {name:'Cookie settings', exact:true}).click();
  await expect(panel(page)).toContainText('Global Privacy Control');
  await panel(page).getByRole('button', {name:'Allow analytics & ads'}).click();
  await expect(panel(page)).toHaveCount(0);
  await expect(page.locator('#anymd-google-tag')).toHaveCount(0);
  expect(google).toEqual([]);
});

test('successful checkout emits begin_checkout, only settled owned purchase emits once', async ({ page }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires loopback Auth and Money fixtures');
  const google = await isolate(page, 'US');
  await page.goto('/sign-in?next=%2Fpricing');
  await page.getByLabel('Email address').fill('test@example.invalid');
  await page.getByRole('button', {name:'Continue', exact:true}).click();
  await page.getByRole('button', {name:'Use password instead'}).click();
  await page.getByLabel('Password').fill('fixture-long-password');
  await page.getByRole('button', {name:'Sign in with password'}).click();
  await expect(page).toHaveURL('/pricing');
  const processorSessionId = 'cs_test_Fixture123';
  const thanks = '/pro/thanks?session_id=' + processorSessionId;
  const origin = new URL(page.url()).origin;
  const verify = (checkoutId: string) => page.request.post('/api/anymd/purchase-measurement', {headers:{origin}, data:{checkoutId, processorSessionId}});
  for (const suffix of ['2', '3']) {
    const result = await verify('chk-' + suffix.repeat(24));
    expect(result.ok()).toBe(true);
    expect(await result.json()).toEqual({purchase:null});
  }
  await page.route('https://checkout.stripe.com/**', route => route.fulfill({status:302, headers:{Location:origin + thanks}}));
  await page.getByRole('checkbox').check();
  const begin = page.waitForRequest('https://checkout.stripe.com/**');
  const measurement = page.waitForResponse('**/api/anymd/purchase-measurement');
  // Capture the consented commands during the brief hosted-checkout redirect.
  await page.addInitScript(() => addEventListener('pagehide', () => {
    const entries = (window as Window & {dataLayer?: ArrayLike<unknown>[]}).dataLayer ?? [];
    const event = entries.map(args => Array.from(args)).find(args => args[0] === 'event' && args[1] === 'begin_checkout');
    if (event) sessionStorage.setItem('test.checkout.event', JSON.stringify(event));
  }));
  await page.reload();
  await page.getByRole('checkbox').check();
  await page.getByRole('button', {name:'Continue to secure checkout'}).click();
  await begin;
  await expect(page).toHaveURL(thanks);
  expect((await measurement).ok()).toBe(true);
  await expect.poll(async () => (await events(page, 'purchase')).length).toBe(1);
  const [purchase] = await events(page, 'purchase');
  expect(purchase[2]).toMatchObject({transaction_id:expect.stringMatching(/^[a-f0-9]{64}$/), currency:'USD', value:29});
  expect(JSON.stringify(purchase)).not.toMatch(/cs_test|checkout_sessions|example\.invalid|usr_fixture|session_id/);
  const start = await page.evaluate(() => JSON.parse(sessionStorage.getItem('test.checkout.event') || 'null'));
  expect(start).toMatchObject(['event', 'begin_checkout', expect.objectContaining({currency:'USD'})]);
  const trace = page.waitForResponse('**/cdn-cgi/trace');
  await page.reload();
  await trace;
  await expect.poll(async () => (await events(page, 'page_view')).length).toBe(1);
  expect(await events(page, 'purchase')).toEqual([]);
  await page.evaluate(id => sessionStorage.setItem('anymd.ga4.checkout.' + id, 'chk-' + '1'.repeat(24)), processorSessionId);
  const replay = page.waitForResponse('**/api/anymd/purchase-measurement');
  await page.reload();
  expect((await replay).ok()).toBe(true);
  await expect.poll(() => page.evaluate(id => sessionStorage.getItem('anymd.ga4.checkout.' + id), processorSessionId)).toBeNull();
  expect(await events(page, 'purchase')).toEqual([]);
  expect(google.every(url => url === tag)).toBe(true);
});
