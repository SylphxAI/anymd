import { createHash } from 'node:crypto';
import { expect, type Page, test } from '@playwright/test';
import { signalChoiceKey } from '../src/lib/consent';
import { fixtureAdsId, fixtureAdsLabel, fixtureApp, fixtureLegacySession, fixtureMoneyParent } from './origins';

const path = '/api/anymd/purchase-conversion';
const sid = 'cs_live_FixturePaid';
const transactionId = (scenario: string) => createHash('sha256').update('anymd-ads-purchase:' + fixtureMoneyParent + '/checkout_sessions/chk-' + scenario).digest('hex').slice(0, 32);
const purchase = {transactionId:transactionId('Paid'), value:29.99, currency:'USD'};
const claimsCookie = '__Secure-anymd_ads_claims';
const ga4 = 'G-R4TD2Z8RR7';

test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Uses only the loopback Money / Auth fixtures and a fake Ads destination');

function params(session_id = sid, ads_consent: unknown = true) {
  return {session_id, ads_consent, amount:999999, currency:'ZZZ'};
}

async function signIn(page: Page) {
  await page.context().addCookies([{name:'anymd_session', value:fixtureLegacySession, url:fixtureApp, secure:true, httpOnly:true}]);
}

async function isolate(page: Page, region = 'DE') {
  const pings: string[] = [];
  await page.context().route(url => /(?:^|\.)(?:google\.com|googletagmanager\.com|google-analytics\.com|googleadservices\.com|doubleclick\.net)$/.test(url.hostname), route => {
    pings.push(route.request().url());
    return route.fulfill({contentType:'application/javascript', body:''});
  });
  await page.route('**/cdn-cgi/trace', route => route.fulfill({body:'loc=' + region + '\n'}));
  return pings;
}

const commands = (page: Page) => page.evaluate(() => (Reflect.get(window, 'dataLayer') as ArrayLike<unknown>[] | undefined)?.map(row => Array.from(row)) ?? []);
const adsCommands = async (page: Page) => (await commands(page)).filter(row =>
  (row[0] === 'config' && row[1] === fixtureAdsId) || (row[0] === 'event' && row[1] === 'conversion'));

test('claims only verified paid live checkout, scopes browser dedup and preserves Money minor units', async ({ page }) => {
  const request = page.request;
  const claim = (data = params(), headers: Record<string, string> = {Origin:fixtureApp}) => request.post(path, {data, headers});
  expect((await claim()).status()).toBe(401);
  await signIn(page);
  for (const headers of [{}, {Origin:'https://example.invalid'}, {Origin:fixtureApp, 'Sec-GPC':'1'}]) {
    expect((await claim(params(), headers)).status()).toBe(403);
  }
  for (const data of [params(sid, false), params(sid, 'true')]) expect((await claim(data)).status()).toBe(403);
  for (const id of ['cs_test_FixturePaid', 'cs_live_bad/path', ...['WrongTenant', 'WrongName', 'WrongUser', 'Unpaid', 'NotLive', 'WrongSid', 'Incomplete', 'Denied', 'BadAmount', 'BadCurrency', 'RedeemUnpaid', 'RedeemCreated'].map(name => 'cs_live_Fixture' + name)]) {
    const response = await claim(params(id));
    expect(response.status(), id).toBe(204);
    expect(response.headers()['cache-control']).toBe('private, no-store');
    expect(response.headers()['set-cookie']).toBeUndefined();
  }
  const result = await claim();
  expect(result.status()).toBe(200);
  expect(result.headers()['cache-control']).toBe('private, no-store');
  expect(await result.json()).toEqual(purchase);
  const [cookie] = (await page.context().cookies()).filter(x => x.name === claimsCookie);
  expect(cookie).toMatchObject({path, httpOnly:true, secure:true, sameSite:'Lax'});
  expect(result.headers()['set-cookie']).not.toContain(sid);
  expect((await claim()).status()).toBe(204);
  const jpy = await claim(params('cs_live_FixtureJPY'));
  expect(await jpy.json()).toEqual({transactionId:transactionId('JPY'), value:4639, currency:'JPY'});
  expect((await claim()).status()).toBe(204);
  // A forged browser claim cannot suppress a verified purchase.
  await page.context().addCookies([{...cookie, value:purchase.transactionId + '~' + '0'.repeat(64)}]);
  expect(await (await claim()).json()).toEqual(purchase);
});

test('the payment return strips its proof and emits one consented Google Ads conversion', async ({ page }) => {
  const pings = await isolate(page);
  await signIn(page);
  const seen: unknown[] = [];
  page.on('request', request => { if (new URL(request.url()).pathname === path) seen.push(request.postDataJSON()); });
  const landing = '/pro/thanks?session_id=' + sid + '&keep=1#main';
  const response = await page.goto(landing);
  expect(await response?.headerValue('content-security-policy')).toContain("default-src 'none'");
  await expect(page.locator('html')).toHaveAttribute('data-google-ads-id', fixtureAdsId);
  await expect(page.locator('html')).toHaveAttribute('data-google-ads-purchase-label', fixtureAdsLabel);
  await expect(page).toHaveURL('/pro/thanks?keep=1#main');
  await expect(page.getByRole('link', {name:'Link a legacy purchase'})).toHaveAttribute('href', '/recover?session_id=' + sid);
  await expect(page.getByRole('button', {name:'Allow analytics & ads'})).toBeEnabled();
  expect(seen).toEqual([]);
  expect(pings).toEqual([]);
  // Upgrade in-place: going through Analytics only must never erase the proof needed for a later grant.
  await page.getByRole('button', {name:'Analytics only'}).click();
  await expect.poll(() => commands(page)).toContainEqual(['config', ga4, expect.objectContaining({send_page_view:false})]);
  expect(await adsCommands(page)).toEqual([]);
  expect(seen).toEqual([]);
  await page.getByRole('button', {name:'Cookie settings', exact:true}).click();
  await page.getByRole('button', {name:'Allow analytics & ads'}).click();
  await expect.poll(() => adsCommands(page)).toEqual([
    ['config', fixtureAdsId, {send_page_view:false}],
    ['event', 'conversion', {send_to:fixtureAdsId + '/' + fixtureAdsLabel, value:purchase.value, currency:'USD', transaction_id:purchase.transactionId,
      page_location:fixtureApp + '/pro', page_referrer:'', page_title:'Anymd'}],
  ]);
  expect(seen).toEqual([{session_id:sid, ads_consent:true}]);
  await page.reload();
  await page.goto(landing);
  await expect.poll(() => adsCommands(page)).toEqual([['config', fixtureAdsId, {send_page_view:false}]]);
  expect(seen).toHaveLength(1);
  // Simulate a new tab without client dedup state: the existing cookie is authoritative.
  const next = await page.context().newPage();
  await next.route('**/cdn-cgi/trace', route => route.fulfill({body:'loc=DE\n'}));
  const claim = next.waitForResponse(url => new URL(url.url()).pathname === path);
  await next.goto(landing);
  expect((await claim).status()).toBe(204);
  expect(await adsCommands(next)).toEqual([['config', fixtureAdsId, {send_page_view:false}]]);
});

for (const [choice, gpc, region] of [['denied', '', 'DE'], ['analytics_only', '', 'DE'], ['granted', 'header', 'US'],
  ['granted', 'navigator', 'US'], ['', '', 'US']] as const) {
  test('no Ads without explicit effective permission: ' + (gpc || choice || 'regional default'), async ({ page }) => {
    const hits = await isolate(page, region);
    await page.addInitScript(({choice, gpc, choiceKey}) => {
      if (gpc === 'navigator') Object.defineProperty(navigator, 'globalPrivacyControl', {value:true});
      if (choice) localStorage.setItem(choiceKey, choice);
    }, {choice, gpc, choiceKey:signalChoiceKey});
    if (gpc === 'header') await page.setExtraHTTPHeaders({'Sec-GPC':'1'});
    const requests: string[] = [];
    page.on('request', request => { if (new URL(request.url()).pathname === path) requests.push(request.url()); });
    await page.goto('/pro/thanks?session_id=' + sid);
    await expect(page).toHaveURL('/pro/thanks');
    await page.getByRole('button', {name:'Cookie settings', exact:true}).click();
    await expect(page.getByRole('button', {name:'Allow analytics & ads'})).toBeEnabled();
    const gaAllowed = choice === 'analytics_only' || !choice;
    if (gaAllowed) await expect.poll(() => hits).toEqual(['https://www.googletagmanager.com/gtag/js?id=' + ga4]);
    else expect(hits).toEqual([]);
    expect(await adsCommands(page)).toEqual([]);
    expect(requests).toEqual([]);
    expect((await page.context().cookies()).some(cookie => cookie.name === claimsCookie)).toBe(false);
  });
}
