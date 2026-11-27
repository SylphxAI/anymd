import { expect, type Page } from '@playwright/test';
import { createHash, randomBytes } from 'node:crypto';
import { consentKey, signalChoiceKey, signalStorage } from '../src/lib/consent';
import { test } from './fixture-test';
import { fixtureAuth, fixtureLegacySession, fixturePublishable } from './origins';

const origin = fixtureAuth;
const batch = origin + '/v1/analytics/events:batch';
const binding = origin + '/v1/events/attribution_bindings';
const utms = {
  utm_source:'wave1', utm_medium:'fixture', utm_campaign:'Signal proof', utm_id:'spring', utm_term:'pdf',
  utm_content:'link', utm_source_platform:'fixture', utm_creative_format:'text', utm_marketing_tactic:'demo',
};
const clickIds = {gclid:'Click_1', gbraid:'Click_2', wbraid:'Click_3', fbclid:'Click_4'};
const landing = '/?' + new URLSearchParams({...utms, ...clickIds, proof:'DO_NOT_SEND'}) + '#PRIVATE_FRAGMENT';
const cors = {'Access-Control-Allow-Origin':'*', 'Access-Control-Allow-Methods':'POST', 'Access-Control-Allow-Headers':'content-type, x-sylphx-key'};
type Event = {event_id:string; event_name:string; properties:Record<string, unknown>};

test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires the loopback platform binding');

async function capture(page: Page, url: string, response: object) {
  const bodies: {consent:string; events:Event[]}[] = [];
  const hits: string[] = [];
  await page.route(url, async route => {
    const request = route.request();
    hits.push(request.method());
    if (request.method() === 'OPTIONS') return route.fulfill({status:204, headers:cors});
    expect(request.method()).toBe('POST');
    expect(request.headers()['x-sylphx-key']).toBe(fixturePublishable);
    expect(request.headers()).not.toHaveProperty('authorization');
    expect(request.headers()).not.toHaveProperty('cookie');
    bodies.push(request.postDataJSON());
    await route.fulfill({status:200, headers:cors, json:response});
  });
  return {hits, bodies, events: (name: string) => bodies.flatMap(body => body.events).filter(event => event.event_name === name)};
}

async function isolate(page: Page, region = 'DE') {
  await page.route('**/cdn-cgi/trace', route => route.fulfill({body:'loc=' + region + '\n'}));
  await page.route(url => /(?:^|\.)(?:googletagmanager|google-analytics|googleadservices|google|doubleclick)\.(?:com|net)$/.test(url.hostname),
    route => route.fulfill({contentType:'application/javascript', body:''}));
  return capture(page, batch, {});
}

const settings = (page: Page) => page.getByRole('button', {name:'Cookie settings', exact:true});

for (const mode of ['unconsented US', 'denied', 'HTTP GPC', 'browser GPC'] as const) {
  test(mode + ' never calls Signal ingest', async ({page}) => {
    const {hits} = await isolate(page, 'US');
    if (mode === 'HTTP GPC') await page.setExtraHTTPHeaders({'Sec-GPC':'1'});
    await page.addInitScript(({key, mode}) => {
      if (mode === 'browser GPC') Object.defineProperty(navigator, 'globalPrivacyControl', {value:true});
      if (mode !== 'unconsented US') localStorage.setItem(key, mode === 'denied' ? 'denied' : 'granted');
    }, {key:signalChoiceKey, mode});
    const response = await page.goto(landing);
    if (mode === 'HTTP GPC') expect(response?.headers().vary?.toLowerCase()).toContain('sec-gpc');
    await settings(page).click();
    await expect(page.getByRole('button', {name:'Reject optional'})).toBeEnabled();
    await page.getByRole('button', {name:'Close without changing'}).click();
    await page.locator('.hero-actions').getByRole('link', {name:/Anymd Pro/}).click();
    await expect(page).toHaveURL(/\/pricing/);
    // Measurement stays off while the Buy path keeps working.
    await page.getByRole('link', {name:'Sign in to buy Pro'}).click();
    await expect(page).toHaveURL(/\/sign-in/);
    if (mode !== 'unconsented US') expect(await page.evaluate(key => localStorage.getItem(key + '.touch'), signalStorage)).toBeNull();
    expect(hits).toEqual([]);
  });
}

test('analytics-only promotes the current route once, keeps all nine UTMs and sends no click IDs or raw URL', async ({page}) => {
  const {events, hits, bodies} = await isolate(page);
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
  const response = await page.goto(landing);
  const csp = response?.headers()['content-security-policy'];
  expect(csp).toContain("default-src 'none'");
  expect(csp).toMatch(/connect-src [^;]*https:\/\/127\.0\.0\.1:3031/);
  await expect(page.getByRole('button', {name:'Analytics only'})).toBeEnabled();
  expect(hits).toEqual([]);
  expect(await page.evaluate(key => localStorage.getItem(key + '.touch'), signalStorage)).toBeNull();
  await page.getByRole('button', {name:'Analytics only'}).click();
  const views = () => events('page_view').map(event => event.properties.path);
  await expect.poll(views).toEqual(['/']);
  expect(events('page_view')[0].properties).toEqual({...utms, path:'/'});
  await page.getByRole('button', {name:'Copy install command', exact:true}).click();
  await expect.poll(() => events('output_copied').map(event => event.properties)).toEqual([
    {...utms, path:'/', surface:'home_install', command_kind:'npm'},
  ]);
  await page.locator('.hero-actions').getByRole('link', {name:/Anymd Pro/}).click();
  await expect.poll(views).toEqual(['/', '/pricing']);
  await page.getByLabel('Billing currency').selectOption('EUR');
  await expect(page).toHaveURL(/currency=EUR/);
  expect(events('pricing_buy_clicked')).toEqual([]);
  await page.getByRole('link', {name:'Sign in to buy Pro'}).click();
  await expect.poll(views).toEqual(['/', '/pricing', '/sign-in']);
  expect(events('pricing_view')).toHaveLength(1);
  // Intent only: one event for the click, nothing for the render or the sign-in page.
  await expect.poll(() => events('pricing_buy_clicked').map(event => event.properties)).toEqual([{...utms, path:'/pricing', entry_surface:'home_hero'}]);
  expect(bodies.every(body => body.consent === 'granted')).toBe(true);
  expect(JSON.stringify(bodies)).not.toMatch(/gclid|gbraid|wbraid|fbclid|DO_NOT_SEND|PRIVATE_FRAGMENT/);
});

test('full grant sends nine UTMs and four click IDs, then binds the verified checkout', async ({page, context, baseURL}) => {
  const {events} = await isolate(page);
  await context.addCookies([{name:'anymd_session', value:fixtureLegacySession, url:baseURL!}]);
  const checkout = 'chk-111111111111111111111111';
  const bindings = await capture(page, binding, {
    binding_id:'fixture', checkout_session_id:checkout, processor_session_id:null, expires_at:'2026-12-01T00:00:00Z',
  });
  await page.route('https://checkout.stripe.com/**', route => route.fulfill({body:'Hosted checkout fixture'}));
  await page.goto('/pricing' + landing.slice(1));
  await page.getByRole('button', {name:'Allow analytics & ads'}).click();
  await expect.poll(() => events('page_view').map(event => event.properties)).toEqual([{...utms, ...clickIds, path:'/pricing'}]);
  await page.getByRole('checkbox').check();
  await page.getByRole('button', {name:'Continue to secure checkout'}).click();
  await expect(page).toHaveURL(/^https:\/\/checkout\.stripe\.com/);
  // Signed-in Continue is the checkout stage, not a second Buy intent.
  expect(events('pricing_buy_clicked')).toEqual([]);
  const expectedLocation = 'https://anymd.sylphx.com/pricing?' + new URLSearchParams(utms);
  expect(bindings.bodies).toEqual([{
    checkout_session_id:checkout, consent:{analytics_storage:'granted', ad_storage:'granted', ad_user_data:'granted', ad_personalization:'granted'},
    ...Object.fromEntries(Object.entries(clickIds).filter(([key]) => key !== 'fbclid')),
    fbc:expect.stringMatching(/^fb\.1\.\d+\.Click_4$/), page_location:expectedLocation,
  }]);
});

test('real checkout BFF batches are idempotent and strictly consent-gated; a Signal 503 cannot stop checkout', async ({page, context, baseURL}) => {
  await context.addCookies([
    {name:'anymd_session', value:fixtureLegacySession, url:baseURL!},
    {name:consentKey, value:'analytics', url:baseURL!},
  ]);
  async function attempt(analyticsGranted: boolean, gpc?: string, replay = false) {
    const checkoutIntent = 'a04ef234' + randomBytes(28).toString('hex');
    const id = 'chk-' + checkoutIntent.slice(8, 32);
    const event = createHash('sha256').update('anymd:checkout_started:' + id).digest('hex');
    const run = async () => {
      const res = await page.request.post('/api/anymd/checkout', {
        headers:{Origin:baseURL!, ...(gpc ? {'Sec-GPC':gpc} : {})},
        data:{currency:'USD', consent:true, analyticsGranted, checkoutIntent, entrySurface:'cli_pdf'},
      });
      expect(res.ok()).toBe(true);
      expect(await res.json()).toMatchObject({url:expect.stringContaining('https://checkout.stripe.com/'), attribution:{checkoutSessionId:id}});
    };
    await run();
    if (replay) await run();
    return async () => (await page.request.get(origin + '/test/checkout-batches?event=' + event)).json();
  }
  const denied = await attempt(false);
  const gpc = await attempt(true, '1');
  const allowed = await attempt(true, undefined, true);
  await expect.poll(async () => (await allowed()).length).toBe(2);
  expect(new Set((await allowed()).map((event: Event) => event.event_id)).size).toBe(1);
  expect(await denied()).toEqual([]);
  expect(await gpc()).toEqual([]);
});
