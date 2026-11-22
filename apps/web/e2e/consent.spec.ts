import { expect, test, type Locator, type Page } from '@playwright/test';

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

test('checkout binds consented campaign data across sign-in without waiting for Events', async ({ page }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires loopback Auth, Money, and Events fixtures');
  await isolate(page, 'US');
  await page.goto('/pricing?gclid=Ad-Click_123');
  await expect(page.getByRole('contentinfo').getByRole('link', {name:'Recover purchase'})).toHaveCount(0);
  await expect(page.getByRole('contentinfo').getByRole('button', {name:'Cookie settings'})).toBeVisible();
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  const client = String(100000000 + Math.floor(Math.random() * 900000000)) + '.1700000000';
  const origin = new URL(page.url()).origin;
  await page.context().addCookies([
    {name:'_ga', value:'GA1.1.' + client, url:origin},
    {name:'_ga_R4TD2Z8RR7', value:'GS2.1.s1700000001$o1$g1', url:origin},
  ]);
  await page.getByRole('link', {name:'Sign in to buy Pro'}).click();
  await page.getByRole('link', {name:'Continue with Google'}).click();
  await expect(page).toHaveURL('/pricing');
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  await page.route('https://checkout.stripe.com/**', route => route.fulfill({status:200, body:'Hosted checkout fixture'}));
  await page.getByRole('checkbox').check();
  const creation = page.waitForResponse('**/api/anymd/checkout');
  await page.getByRole('button', {name:'Continue to secure checkout'}).click();
  expect((await creation).ok()).toBe(true);
  await expect.poll(async () => (await page.request.get('http://127.0.0.1:3031/test/attribution?client=' + client)).json()).toEqual({
    responseStarted:false,
    body:{checkout_session_id:'chk-111111111111111111111111', consent:all,
      ga_client_id:client, ga_session_id:'1700000001', gclid:'Ad-Click_123', page_location:'https://anymd.sylphx.com/pricing'},
  });
  await expect(page).toHaveURL(url => url.origin === 'https://checkout.stripe.com');
});

test('consented checkout emits begin_checkout but the return never measures a purchase', async ({ page }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires loopback Auth and Money fixtures');
  const google = await isolate(page, 'US');
  const measurementRequests: string[] = [];
  page.on('request', request => {
    if (new URL(request.url()).pathname === '/api/anymd/purchase-measurement') measurementRequests.push(request.url());
  });
  // Save the checkout event before the redirect clears the pricing page's queue.
  await page.addInitScript(() => addEventListener('pagehide', () => {
    const entries = (window as Window & {dataLayer?: ArrayLike<unknown>[]}).dataLayer ?? [];
    const event = entries.map(args => Array.from(args)).find(args => args[0] === 'event' && args[1] === 'begin_checkout');
    if (event) sessionStorage.setItem('test.checkout.event', JSON.stringify(event));
  }));
  await page.goto('/sign-in?next=%2Fpricing');
  await page.getByLabel('Email address').fill('test@example.invalid');
  await page.getByRole('button', {name:'Continue', exact:true}).click();
  await page.getByLabel('6-digit code').fill('123456');
  await expect(page).toHaveURL('/pricing');
  await expect.poll(async () => (await events(page, 'pricing_view')).length).toBe(1);
  const processorSessionId = 'cs_test_Fixture123';
  const thanks = '/pro/thanks?session_id=' + processorSessionId;
  const origin = new URL(page.url()).origin;
  await page.route('https://checkout.stripe.com/**', route => route.fulfill({status:302, headers:{Location:origin + thanks}}));
  await page.getByRole('checkbox').check();
  const checkout = page.waitForResponse(response => new URL(response.url()).pathname === '/api/anymd/checkout');
  await page.getByRole('button', {name:'Continue to secure checkout'}).click();
  const response = await checkout;
  expect(response.ok()).toBe(true);
  expect(await response.json()).toEqual({url:'https://checkout.stripe.com/c/pay/' + processorSessionId});
  await expect(page).toHaveURL(thanks);
  await expect(page.getByRole('heading', {name:'Activate Anymd Pro'})).toBeVisible();
  await expect.poll(async () => (await events(page, 'page_view')).length).toBe(1);
  expect(await events(page, 'purchase')).toEqual([]);
  const start = await page.evaluate(() => JSON.parse(sessionStorage.getItem('test.checkout.event') || 'null'));
  expect(start).toMatchObject(['event', 'begin_checkout', expect.objectContaining({currency:'USD'})]);
  expect(await page.evaluate(() => Object.keys(sessionStorage).some(key => key.startsWith('anymd.ga4.checkout.')))).toBe(false);

  // A tab opened before the change may still carry the old local checkout state.
  await page.evaluate(id => sessionStorage.setItem('anymd.ga4.checkout.' + id, 'chk-' + '1'.repeat(24)), processorSessionId);
  await page.reload();
  await expect(page.getByRole('heading', {name:'Activate Anymd Pro'})).toBeVisible();
  await expect.poll(async () => (await events(page, 'page_view')).length).toBe(1);
  expect(await events(page, 'purchase')).toEqual([]);
  expect(measurementRequests).toEqual([]);
  expect(google.every(url => url === tag)).toBe(true);

  const retired = await page.request.post('/api/anymd/purchase-measurement', {headers:{origin}, data:{}});
  expect(retired.status()).toBe(404);
});

function boxesOverlap(a: { x: number; y: number; width: number; height: number }, b: { x: number; y: number; width: number; height: number }) {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

test('the compact consent bar leaves conversion controls reachable without a choice', async ({ page }) => {
  const google = await isolate(page, 'GB');
  const targets: [string, () => Locator[]][] = [
    ['/', () => [page.getByLabel('Example Markdown'), page.getByRole('link', {name:'Try Studio Free'}), page.getByRole('link', {name:'Get Anymd Pro — $29'})]],
    ['/pricing', () => [page.getByRole('link', {name:'Install MCP / CLI'}), page.getByRole('link', {name:'Sign in to buy Pro'})]],
    ['/studio', () => [page.getByRole('link', {name:'Get Anymd Pro — $29 once', exact:true})]],
  ];
  const mobile = page.viewportSize()!.width < 800;
  for (const [path, controls] of targets) {
    await page.goto(path);
    const bar = panel(page);
    await expect(bar).toBeVisible();
    const box = (await bar.boundingBox())!;
    expect(box.height, path).toBeLessThanOrEqual(mobile ? 104 : 72);
    expect(await page.locator('#app').evaluate(el => Number.parseInt(getComputedStyle(el).paddingBottom, 10))).toBeGreaterThanOrEqual(box.height);
    const items = controls();
    for (const control of items) {
      await expect(control).toBeVisible();
      expect.soft(boxesOverlap((await control.boundingBox())!, box), `${path} ${await control.textContent()} at rest`).toBe(false);
    }
    for (const control of items) {
      await control.evaluate(el => el.scrollIntoView({block:'center'}));
      expect(boxesOverlap((await control.boundingBox())!, (await bar.boundingBox())!)).toBe(false);
      if ((await control.getAttribute('href')) !== null) await control.click({trial:true});
    }
    const buttons = bar.getByRole('button');
    await expect(buttons).toHaveCount(3);
    const styles = await buttons.evaluateAll(els => els.map(el => {
      const style = getComputedStyle(el);
      return [style.borderTopColor, style.borderTopWidth, style.backgroundColor, style.color, style.fontWeight];
    }));
    expect(styles[0][1]).toBe('1px');
    expect(styles.every(style => JSON.stringify(style) === JSON.stringify(styles[0]))).toBe(true);
    await page.getByRole('button', { name: 'Cookie settings', exact: true }).click();
    await expect(bar.getByRole('button', { name: 'Reject optional' })).toBeFocused();
    await bar.getByRole('button', { name: 'Close without changing' }).click();
  }
  await expect(page.locator('#anymd-google-tag')).toHaveCount(0);
  expect(google).toEqual([]);
});
