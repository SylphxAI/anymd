import { expect, type Locator, type Page } from '@playwright/test';
import { test } from './fixture-test';
import { randomBytes } from 'node:crypto';
import { signupCookie } from '../src/lib/signup';

const tag = 'https://www.googletagmanager.com/gtag/js?id=G-R4TD2Z8RR7';
const googleHost = /(?:^|\.)(?:googletagmanager\.com|google-analytics\.com|googleadservices\.com|doubleclick\.net|google\.com)$/;
const denied = {analytics_storage:'denied', ad_storage:'denied', ad_user_data:'denied', ad_personalization:'denied'};
const analyticsOnly = {...denied, analytics_storage:'granted'};
const all = {analytics_storage:'granted', ad_storage:'granted', ad_user_data:'granted', ad_personalization:'granted'};
const eventsOrigin = 'https://api.sylphx.com';
const eventsEndpoint = eventsOrigin + '/v1/events/attribution_bindings';
const checkoutSessionId = 'chk-111111111111111111111111';
const apiKey = ['sylphx', 'pk', 'test', 'fixture'].join('_');
const fbc = 'fb.1.1700000000000.Other_Click';
const fbp = 'fb.1.1700000000000.12345';

async function captureBindings(page: Page) {
  const bindings: unknown[] = [];
  await page.route(eventsEndpoint, async route => {
    if (route.request().method() === 'OPTIONS') {
      await route.fulfill({status:204, headers:{'Access-Control-Allow-Origin':'*', 'Access-Control-Allow-Methods':'POST', 'Access-Control-Allow-Headers':'content-type, x-sylphx-key'}});
      return;
    }
    expect(route.request().method()).toBe('POST');
    expect(route.request().headers()['x-sylphx-key']).toBe(apiKey);
    bindings.push(route.request().postDataJSON());
    // An Events failure must not stop checkout.
    await route.fulfill({status:503, headers:{'Access-Control-Allow-Origin':'*'}});
  });
  return bindings;
}

async function setAttributionCookies(page: Page) {
  const url = new URL(page.url()).origin;
  await page.context().addCookies([
    {name:'_ga', value:'GA1.1.123.1700000000', url},
    {name:'_ga_R4TD2Z8RR7', value:'GS2.1.s1700000001$o1$g1', url},
    {name:'_ga_OTHER', value:'GS1.1.700', url},
    {name:'_fbc', value:fbc, url}, {name:'_fbp', value:fbp, url},
  ]);
}

async function isolate(page: Page, country: string | null, gpc = false) {
  const requests: string[] = [];
  await page.context().route(url => googleHost.test(url.hostname), route => {
    requests.push(route.request().url());
    // Never load Google code. Leave the app's actual gtag queue inspectable.
    return route.fulfill({contentType:'application/javascript', body:''});
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

async function moneyCheckoutMetadata(page: Page, checkoutIntent: string) {
  const url = new URL('https://127.0.0.1:3031/test/checkout-metadata');
  url.searchParams.set('intent', checkoutIntent);
  const response = await page.request.get(url.href);
  expect(response.ok()).toBe(true);
  return response.json();
}

const optInRegions = [
  ['EEA', 'DE'], ['UK', 'GB'], ['Switzerland', 'CH'], ['unknown', null],
  ['Åland', 'AX'], ['French Guiana', 'GF'], ['Guadeloupe', 'GP'], ['Martinique', 'MQ'],
  ['Saint Martin', 'MF'], ['Réunion', 'RE'], ['Mayotte', 'YT'],
] as const;

for (const [label, code] of optInRegions) {
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
  await expect.poll(() => page.evaluate(() => localStorage.getItem('anymd.signal.v1.consent'))).toBe('analytics_only');
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
  await expect(page.getByRole('main')).toContainText('23 November 2026');
  expect(google).toEqual([tag]);
});

for (const country of ['US','CA','AU']) {
  test(country + ' starts with all four permissions without a banner click', async ({ page }) => {
    const google = await isolate(page, country);
    await page.goto('/pricing');
    await expect.poll(() => google).toEqual([tag]);
    await expect(panel(page)).toHaveCount(0);
    expect((await commands(page))[0]).toEqual(['consent','default',all]);
    expect(await events(page, 'pricing_view')).toHaveLength(1);
  });
}

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

test('HTTP Global Privacy Control alone keeps the Google tag off', async ({ page }) => {
  const google = await isolate(page, 'CA');
  await page.setExtraHTTPHeaders({'Sec-GPC':'1'});
  await page.goto('/pricing');
  await expect(page.locator('html')).toHaveAttribute('data-signal-gpc', '1');
  await page.getByRole('button', {name:'Cookie settings', exact:true}).click();
  await expect(panel(page)).toContainText('Global Privacy Control');
  await panel(page).getByRole('button', {name:'Allow analytics & ads'}).click();
  expect(await commands(page)).toEqual([]);
  expect(google).toEqual([]);
});

test('an existing rejection survives the first load in an opt-out region', async ({ page }) => {
  const google = await isolate(page, 'AU');
  await page.addInitScript(() => localStorage.setItem('anymd.cookie-settings.v1', 'reject'));
  await page.goto('/pricing?gclid=Do_Not_Store');
  await expect.poll(() => page.evaluate(() => localStorage.getItem('anymd.signal.v1.consent'))).toBe('denied');
  expect(await page.evaluate(() => localStorage.getItem('anymd.signal.v1.touch'))).toBeNull();
  expect(google).toEqual([]);
});

test('checkout works with no consent and sends no GA commands or analytics storage writes', async ({ page }) => {
  const hits = await isolate(page, 'FR');
  await page.addInitScript(() => {
    const writes: string[] = [];
    const setItem = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) { writes.push(key); return setItem.call(this, key, value); };
    Object.defineProperty(window, '__storageWrites', {value:writes});
  });
  const bindings = await captureBindings(page);
  await page.route('**/api/auth/session*', route => route.fulfill({json:{authConfigured:true, user:{name:'Test', email:'test@example.invalid'}}}));
  await page.route('**/api/anymd/checkout', async route => {
    expect(route.request().postDataJSON()).toMatchObject({entrySurface:'studio_cite_preview'});
    expect(await commands(page)).toEqual([]);
    expect(await page.evaluate(() => Reflect.get(window, '__storageWrites'))).toEqual([]);
    await route.fulfill({json:{url:'/pro/thanks?session_id=cs_test_NOT_FOR_GOOGLE', attribution:{checkoutSessionId, apiKey, baseUrl:eventsOrigin, gpc:false}}});
  });
  await page.goto('/pricing?entry_surface=studio_cite_preview');
  await expect(panel(page)).toBeVisible();
  await expect(page.getByRole('checkbox')).toBeVisible();
  // Hide the banner with the test DOM only: no preference is saved.
  await panel(page).evaluate(element => { element.style.display = 'none'; });
  await page.getByRole('checkbox').check();
  await page.getByRole('button', {name:'Continue to secure checkout'}).click();
  await expect(page).toHaveURL('/pro/thanks?session_id=cs_test_NOT_FOR_GOOGLE');
  await expect(panel(page)).toBeVisible();
  expect(await commands(page)).toEqual([]);
  expect(await page.evaluate(() => ({writes:Reflect.get(window, '__storageWrites'), gtag:typeof Reflect.get(window, 'gtag')}))).toEqual({writes:[], gtag:'undefined'});
  expect(hits).toEqual([]);
  expect(bindings).toEqual([{checkout_session_id:checkoutSessionId, consent:denied}]);
});

test('implicit US consent binds complete attribution after real sign-in and Money checkout', async ({ page }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires loopback Auth and Money fixtures');
  await isolate(page, 'US');
  const bindings = await captureBindings(page);
  await page.route('**/api/anymd/checkout', async route => {
    const response = await route.fetch();
    const json = await response.json();
    expect(json.attribution).toEqual({checkoutSessionId, apiKey, baseUrl:'https://127.0.0.1:3031', gpc:false});
    // The SDK requires HTTPS. Point it at the browser-intercepted Events origin.
    await route.fulfill({response, json:{...json, attribution:{...json.attribution, baseUrl:eventsOrigin}}});
  });
  await page.goto('/pricing?gclid=Ad-Click_123&gbraid=Braided_1&wbraid=Braided_2&utm_source=search&currency=HKD&proof=private');
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  await expect(panel(page)).toHaveCount(0);
  await setAttributionCookies(page);
  await page.getByRole('link', {name:'Sign in to buy Pro'}).click();
  await page.getByRole('link', {name:'Continue with Google'}).click();
  await expect(page).toHaveURL('/pricing?currency=HKD');
  await expect(page.getByLabel('Billing currency')).toHaveValue('HKD');
  await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
  await page.route('https://checkout.stripe.com/**', route => route.fulfill({status:200, body:'Hosted checkout fixture'}));
  await page.getByRole('checkbox').check();
  const creation = page.waitForResponse('**/api/anymd/checkout');
  await page.getByRole('button', {name:'Continue to secure checkout'}).click();
  const created = await creation;
  expect(created.ok()).toBe(true);
  expect(created.request().postDataJSON()).toMatchObject({currency:'HKD'});
  await expect(page).toHaveURL(url => url.origin === 'https://checkout.stripe.com');
  expect(bindings).toEqual([{
    checkout_session_id:checkoutSessionId, consent:all,
    ga_client_id:'123.1700000000', ga_session_id:'1700000001', gclid:'Ad-Click_123', gbraid:'Braided_1', wbraid:'Braided_2', fbc, fbp,
    page_location:'https://anymd.sylphx.com/pricing?utm_source=search',
  }]);
});

for (const [label, choice, serverGpc, expected] of [
  ['Canada implicit grant', null, false, all],
  ['Australia implicit grant', null, false, all],
  ['explicit rejection', 'Reject optional', false, denied],
  ['analytics-only opt-in', 'Analytics only', false, analyticsOnly],
  ['server GPC override', 'Allow analytics & ads', true, denied],
] as const) {
  test('browser checkout: ' + label, async ({ page }) => {
    const bindings = await captureBindings(page);
    await isolate(page, label.startsWith('Australia') ? 'AU' : 'CA');
    await page.route('**/api/auth/session*', route => route.fulfill({json:{authConfigured:true, user:{name:'Test', email:'test@example.invalid'}}}));
    await page.route('**/api/anymd/checkout', route => route.fulfill({json:{url:'/pro/thanks?session_id=cs_test_Fixture123', attribution:{checkoutSessionId, apiKey, baseUrl:eventsOrigin, gpc:serverGpc}}}));
    await page.goto('/pricing?gclid=Ad-Click_123&utm_source=search&proof=private');
    await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
    if (choice) {
      await page.getByRole('button', {name:'Cookie settings', exact:true}).click();
      await panel(page).getByRole('button', {name:choice}).click();
      await expect(page.locator('#anymd-google-tag')).toHaveCount(choice === 'Reject optional' ? 0 : 1);
      await expect(panel(page)).toHaveCount(0);
    }
    await setAttributionCookies(page);
    await page.getByRole('checkbox').check();
    await page.getByRole('button', {name:'Continue to secure checkout'}).click();
    await expect(page).toHaveURL('/pro/thanks?session_id=cs_test_Fixture123');
    expect(bindings).toEqual([{
      checkout_session_id:checkoutSessionId, consent:expected,
      ...(expected.analytics_storage === 'granted' && {ga_client_id:'123.1700000000', ga_session_id:'1700000001', page_location:'https://anymd.sylphx.com/pricing?utm_source=search'}),
      ...(expected.ad_storage === 'granted' && {gclid:'Ad-Click_123', fbc, fbp}),
    }]);
  });
}

test('checkout accepts only existing funnel labels in Money metadata', async ({ page }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires loopback Auth and Money fixtures');
  const google = await isolate(page, 'FR', true);
  await page.goto('/pricing');
  await page.getByRole('link', {name:'Sign in to buy Pro'}).click();
  await page.getByRole('link', {name:'Continue with Google'}).click();
  await expect(page).toHaveURL('/pricing');
  const origin = new URL(page.url()).origin;
  for (const [entrySurface, expected] of [
    ['cli_video', 'cli_video'], ['pricing', 'pricing'], ['pro', 'pro'],
    ['studio_cite_preview', 'studio_cite_preview'], ['seo_vs-marker', 'seo_vs-marker'],
    [undefined, 'pricing'], ['billing', 'pricing'], ['cli_unrecognized', 'pricing'],
    ['private@example.invalid', 'pricing'], ['cli_pdf\n', 'pricing'], ['cli_vidéo', 'pricing'],
    ['x'.repeat(101), 'pricing'], [null, 'pricing'], [{entry_surface:'cli_pdf'}, 'pricing'],
  ] as const) {
    const checkoutIntent = randomBytes(32).toString('hex');
    const response = await page.request.post('/api/anymd/checkout', {
      headers:{origin, 'sec-gpc':'1'},
      data:{currency:'USD', consent:true, checkoutIntent, entrySurface, metadata:{customer:'private@example.invalid'}},
    });
    expect(response.ok()).toBe(true);
    expect(await moneyCheckoutMetadata(page, checkoutIntent)).toEqual({entry_surface:expected});
  }
  expect(google).toEqual([]);
});

test('consented checkout emits begin_checkout but the return never measures a purchase', async ({ page }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires loopback Auth and Money fixtures');
  const google = await isolate(page, 'FR');
  await captureBindings(page);
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
  await page.goto('/pricing?entry_surface=cli_pdf&email=private%40example.invalid');
  await panel(page).getByRole('button', {name:'Analytics only'}).click();
  await page.getByRole('link', {name:'Sign in to buy Pro'}).click();
  await page.getByLabel('Email address').fill('test@example.invalid');
  await page.getByRole('button', {name:'Continue', exact:true}).click();
  await page.getByLabel('6-digit code').fill('123456');
  await expect(page).toHaveURL('/pricing?entry_surface=cli_pdf');
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
  expect(await response.json()).toMatchObject({url:'https://checkout.stripe.com/c/pay/' + processorSessionId});
  const {checkoutIntent} = response.request().postDataJSON();
  expect(await moneyCheckoutMetadata(page, checkoutIntent)).toEqual({entry_surface:'cli_pdf'});
  await expect(page).toHaveURL(thanks);
  await expect(page.getByRole('heading', {name:'Activate Anymd Pro'})).toBeVisible();
  await expect.poll(async () => (await events(page, 'page_view')).length).toBe(1);
  expect(await events(page, 'purchase')).toEqual([]);
  const start = await page.evaluate(() => JSON.parse(sessionStorage.getItem('test.checkout.event') || 'null'));
  expect(start).toEqual(['event', 'begin_checkout', {
    value:29, currency:'USD', plan_sku:'anymd_pro', entry_surface:'cli_pdf',
    items:[{item_id:'anymd_pro', item_name:'Anymd Pro', quantity:1, price:29}],
    event_timeout:250, page_location:origin + '/pricing', page_referrer:'', page_title:'Anymd', send_to:'G-R4TD2Z8RR7',
  }]);
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

async function watchStorage(page: Page) {
  await page.addInitScript(() => {
    const writes: string[] = [];
    const setItem = Storage.prototype.setItem;
    Storage.prototype.setItem = function (key, value) { writes.push(key); return setItem.call(this, key, value); };
    const cookie = Object.getOwnPropertyDescriptor(Document.prototype, 'cookie')!;
    Object.defineProperty(document, 'cookie', {get: () => cookie.get!.call(document), set: value => {
      writes.push(value.split('=', 1)[0]);
      cookie.set!.call(document, value);
    }});
    Object.defineProperty(window, '__storageWrites', {value:writes});
  });
}

// In production, stub the web Auth callback. Locally, exercise the real
// callback and verification routes against the loopback Sylphx Auth fixture.
for (const [scenario, choice, country, gpc] of [
  ['new', 'analytics', 'FR', false],
  ['returning', 'analytics', 'FR', false],
  ['legacy', 'analytics', 'FR', false],
  ['new', null, 'FR', false],
  ['new', 'reject', 'FR', false],
  ['new', null, 'US', false],
  ['new', 'analytics', 'US', true],
] as const) {
  const label = [scenario, choice ?? 'no-choice', country, gpc ? 'GPC' : ''].filter(Boolean).join(' ');
  test.describe('OAuth signup: ' + label, () => {
    const live = Boolean(process.env.PLAYWRIGHT_BASE_URL);
    test.use({userAgent:live ? undefined : 'anymd-fixture-' + scenario + '-user', extraHTTPHeaders:gpc ? {'Sec-GPC':'1'} : {}});
    test('checks the handoff and measurement', async ({ page }) => {
      const google = await isolate(page, country, gpc);
      await watchStorage(page);
      const signUp = scenario === 'new' && choice === 'analytics' && !gpc;
      const gaAllowed = !gpc && (choice === 'analytics' || (choice === null && country === 'US'));
      const next = '/pricing?entry_surface=cli_video&email=private%40example.invalid#private';
      if (live) {
        await page.route('**/api/auth/**', route => {
          if (new URL(route.request().url()).pathname === '/api/auth/callback') {
            const headers: Record<string, string> = {Location:next};
            if (signUp) headers['Set-Cookie'] = signupCookie + '=google.cli_video; Path=/; Max-Age=60; Secure; SameSite=Lax';
            return route.fulfill({status:303, headers});
          }
          return route.fulfill({status:200, json:{authConfigured:true, user:null, providers:[]}});
        });
      }
      await page.goto('/pricing');
      if (choice) {
        await page.getByRole('button', {name:'Cookie settings', exact:true}).click();
        await panel(page).getByRole('button', {name:choice === 'reject' ? 'Reject optional' : 'Analytics only'}).click();
      }
      const callback = page.waitForResponse(response => new URL(response.url()).pathname === '/api/auth/callback');
      const trace = page.waitForResponse('**/cdn-cgi/trace');
      const landing = await page.goto(live ? '/api/auth/callback' : '/sign-in?next=' + encodeURIComponent(next));
      if (!live) await page.getByRole('link', {name:'Continue with Google'}).click();
      const response = await callback;
      expect(response.status()).toBe(303);
      if (live) {
        // Chromium applies the stub's cookie even though Playwright omits
        // Set-Cookie from the fulfilled response. Check the landing request.
        expect(landing?.ok()).toBe(true);
        const jar = await landing?.request().headerValue('cookie');
        const handoff = (jar ?? '').split(';').map(cookie => cookie.trim()).filter(cookie => cookie.startsWith(signupCookie + '='));
        expect(handoff).toEqual(signUp ? [signupCookie + '=google.cli_video'] : []);
      } else {
        const handoff = (await response.headerValues('set-cookie')).filter(value => value.startsWith(signupCookie + '='));
        if (signUp) expect(handoff).toEqual([expect.stringMatching(/^anymd_signup=google\.cli_video;/)]);
        else expect(handoff).toEqual([]);
      }
      await trace;
      await expect(page).toHaveURL(next);
      if (gaAllowed) await expect.poll(async () => (await events(page, 'page_view')).length).toBe(1);
      else if (choice === null && !gpc) await expect(panel(page)).toBeVisible();
      else await expect(page.locator('#anymd-google-tag')).toHaveCount(0);
      const origin = new URL(page.url()).origin;
      expect(await events(page, 'sign_up')).toEqual(signUp ? [['event', 'sign_up', {
        method:'google', entry_surface:'cli_video', page_location:origin + '/pricing', page_referrer:'', page_title:'Anymd', send_to:'G-R4TD2Z8RR7',
      }]] : []);
      expect((await page.context().cookies()).some(cookie => cookie.name === signupCookie)).toBe(false);
      if (!signUp) expect(await page.evaluate(() => Reflect.get(window, '__storageWrites'))).toEqual([]);
      if (!gaAllowed) {
        expect(await commands(page)).toEqual([]);
        expect(await page.evaluate(() => typeof Reflect.get(window, 'gtag'))).toBe('undefined');
        expect(google).toEqual([]);
      }
      if (signUp) {
        await page.getByRole('contentinfo').getByRole('link', {name:'Privacy', exact:true}).click();
        await expect(page).toHaveURL('/privacy');
        await expect.poll(async () => (await events(page, 'page_view')).length).toBe(2);
        expect(await events(page, 'sign_up')).toHaveLength(1);
        await page.reload();
        await expect.poll(async () => (await events(page, 'page_view')).length).toBe(1);
        expect(await events(page, 'sign_up')).toEqual([]);
      }
      expect(google.every(url => url === tag)).toBe(true);
    });
  });
}

test('email OTP signup uses the redeemed ticket and drops private entry surfaces', async ({ page, context }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires loopback Auth fixture');
  await isolate(page, 'FR');
  await context.setExtraHTTPHeaders({'user-agent':'anymd-fixture-new-user'});
  await page.goto('/sign-in?next=' + encodeURIComponent('/pro?entry_surface=private@example.invalid'));
  await panel(page).getByRole('button', {name:'Analytics only'}).click();
  await page.getByLabel('Email address').fill('test@example.invalid');
  await page.getByRole('button', {name:'Continue', exact:true}).click();
  await page.getByLabel('6-digit code').fill('123456');
  await expect(page).toHaveURL('/pro?entry_surface=private@example.invalid');
  const origin = new URL(page.url()).origin;
  await expect.poll(() => events(page, 'sign_up')).toEqual([['event', 'sign_up', {
    method:'email_otp', entry_surface:'pro', page_location:origin + '/pro', page_referrer:'', page_title:'Anymd', send_to:'G-R4TD2Z8RR7',
  }]]);
  await page.reload();
  await expect.poll(async () => (await events(page, 'page_view')).length).toBe(1);
  expect(await events(page, 'sign_up')).toEqual([]);
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

const campaign = {
  gclid:'Synthetic-Click_1', gbraid:'Synthetic-Braid_2', wbraid:'Synthetic-Braid_3',
  utm_source:'search', utm_medium:'cpc', utm_campaign:'PDF & video', utm_term:'bounding box', utm_content:'frame proof',
};

for (const acceptOn of ['/pricing', '/sign-in']) {
  test('retains campaign through internal links and captures only on opt-in at ' + acceptOn, async ({ page }) => {
    const google = await isolate(page, 'FR');
    await page.addInitScript(() => {
      const writes: string[] = [];
      const setItem = Storage.prototype.setItem;
      Storage.prototype.setItem = function (key, value) { writes.push(key); setItem.call(this, key, value); };
      Object.defineProperty(window, '__campaignWrites', {value:writes});
    });
    const query = new URLSearchParams({...campaign, unrelated:'do-not-forward'});
    await page.goto('/?' + query);
    const banner = panel(page);
    await expect(banner.getByRole('button', {name:'Allow analytics & ads'})).toBeEnabled();
    const desktop = page.viewportSize()!.width > 690;
    if (!desktop) await page.locator('summary[aria-label="Open navigation"]').click();
    await page.getByRole('navigation', {name:desktop ? 'Main' : 'Mobile', exact:true}).getByRole('link', {name:'Pricing'}).click();
    await expect(page).toHaveURL(url => url.pathname === '/pricing');
    expect(Object.fromEntries(new URL(page.url()).searchParams)).toEqual(campaign);
    if (acceptOn === '/sign-in') {
      await page.getByRole('link', {name:'Sign in to buy Pro'}).click();
      await expect(page).toHaveURL(url => url.pathname === '/sign-in');
      expect(Object.fromEntries(new URL(page.url()).searchParams)).toEqual({next:'/pricing', ...campaign});
      // Hard reload simulates following the same SSR-rendered link in a new tab.
      await page.reload();
      await expect(banner.getByRole('button', {name:'Allow analytics & ads'})).toBeEnabled();
    }
    expect(await page.evaluate(() => Reflect.get(window, '__campaignWrites'))).toEqual([]);
    expect((await page.context().cookies()).filter(cookie => /^(?:_ga|_gcl_|anymd\.cookie|anymd_attribution)/.test(cookie.name))).toEqual([]);
    expect(google).toEqual([]);
    await banner.getByRole('button', {name:'Allow analytics & ads'}).click();
    await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('anymd.signal.v1.touch') ?? 'null')?.touch)).toEqual(campaign);
    await expect.poll(() => google).toEqual([tag]);
  });
}

test('queues one sanitized manual page_view per path, not for search-only changes', async ({ page }) => {
  await isolate(page, 'FR');
  await page.goto('/?utm_source=synthetic');
  await panel(page).getByRole('button', {name:'Analytics only'}).click();
  const origin = new URL(page.url()).origin;
  const views = async () => (await events(page, 'page_view')).map(x => (x[2] as {page_location:string}).page_location);
  await expect.poll(views).toEqual([origin + '/']);
  const desktop = page.viewportSize()!.width > 690;
  if (!desktop) await page.locator('summary[aria-label="Open navigation"]').click();
  await page.getByRole('navigation', {name:desktop ? 'Main' : 'Mobile', exact:true}).getByRole('link', {name:'Pricing'}).click();
  await expect.poll(views).toEqual([origin + '/', origin + '/pricing']);
  await page.getByLabel('Billing currency').selectOption('EUR');
  await expect(page).toHaveURL('/pricing?currency=EUR');
  expect(await views()).toEqual([origin + '/', origin + '/pricing']);
  await page.getByRole('link', {name:'Sign in to buy Pro'}).click();
  await expect.poll(views).toEqual([origin + '/', origin + '/pricing', origin + '/sign-in']);
});
