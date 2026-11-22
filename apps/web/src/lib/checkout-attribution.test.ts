import assert from 'node:assert/strict';
import { test } from 'node:test';
import { bindCheckoutAttribution } from './checkout-attribution';

const id = 'chk-0123456789abcdef01234567';
const money = {origin:'https://events.example.invalid', pk:['sylphx', 'pk', 'test', 'fixture'].join('_')};
const cookieKey = 'anymd.cookie-settings.v1';
const ga = '_ga=GA1.1.123456789.1700000000';
const gs1 = '_ga_R4TD2Z8RR7=GS1.1.1700000001.2.1.1700000002.0.0.0';
const gs2 = '_ga_R4TD2Z8RR7=' + encodeURIComponent('GS2.1.o2$g1$s1700000001$t1700000002$h0');
const adIds = {gclid:'aBc_123-0', gbraid:'g.123', wbraid:'w-123'};
const meta = {fbc:'fb.1.1700000000000.Click_123', fbp:'fb.1.1700000000000.123456789'};
const clicks = 'anymd_attribution=' + encodeURIComponent(JSON.stringify(adIds));
const ads = clicks + '; _fbc=' + meta.fbc + '; _fbp=' + meta.fbp;
const analytics = {analytics_storage:'granted', ad_storage:'denied', ad_user_data:'denied', ad_personalization:'denied'};
const all = {...analytics, ad_storage:'granted', ad_user_data:'granted'};
const page = {checkout_session_id:id, page_location:'https://anymd.sylphx.com/pricing'};
const ids = {ga_client_id:'123456789.1700000000', ga_session_id:'1700000001'};

function headers(cookie = '', extra: HeadersInit = {}) {
  return new Headers({cookie, referer:'https://example.invalid/billing?secret=fake#private', ...extra});
}

test('never binds on missing, invalid or rejected consent, GPC, or an invalid Money ID', async t => {
  const post = t.mock.method(globalThis, 'fetch', async () => new Response(null, {status:204}));
  for (const choice of [undefined, null, {}, true, '', 'granted', 'reject', 'essential-only']) {
    await bindCheckoutAttribution(money, headers(ga + '; ' + gs1), id, choice);
  }
  for (const value of ['1', '0, 1']) {
    await bindCheckoutAttribution(money, headers(ga + '; ' + cookieKey + '=all', {'sec-gpc':value}), id, 'all');
  }
  for (const cookie of ['reject', '', '%FF', 'all; ' + cookieKey + '=all', 'all; ' + cookieKey + '=reject']) {
    await bindCheckoutAttribution(money, headers(ga + '; ' + cookieKey + '=' + cookie), id, 'all');
  }
  await bindCheckoutAttribution(money, headers(cookieKey + '=all'), id, undefined);
  await bindCheckoutAttribution(money, headers(ga), 'cs_test_not_a_money_id', 'all');
  assert.equal(post.mock.callCount(), 0);
});

test('posts both GA session formats with the Money key and a canonical page, not request headers', async t => {
  const sends: Array<[RequestInfo | URL, RequestInit | undefined]> = [];
  t.mock.method(globalThis, 'fetch', async (url: RequestInfo | URL, options?: RequestInit) => {
    sends.push([url, options]);
    return new Response(null, {status:204});
  });
  for (const session of [gs1, gs2]) await bindCheckoutAttribution(money, headers([ga, session, ads].join('; ')), id, 'all');
  assert.equal(sends.length, 2);
  for (const [url, options] of sends) {
    assert.equal(url, 'https://events.example.invalid/v1/events/attribution_bindings');
    assert.equal(options?.method, 'POST');
    assert.equal(options.redirect, 'error');
    assert.equal(options.cache, 'no-store');
    assert.deepEqual(options.headers, {'Content-Type':'application/json', 'X-Sylphx-Key':money.pk});
    assert.deepEqual(JSON.parse(String(options.body)), {...page, consent:all, ...ids, ...adIds, ...meta});
  }
});

test('analytics-only choice or saved restriction strips every ad identifier', async t => {
  const sends: unknown[] = [];
  t.mock.method(globalThis, 'fetch', async (_url: string, options: RequestInit) => {
    sends.push(JSON.parse(String(options.body))); return new Response(null, {status:204});
  });
  await bindCheckoutAttribution(money, headers([ga, gs1, ads, cookieKey + '=all'].join('; ')), id, 'analytics');
  await bindCheckoutAttribution(money, headers([ga, gs1, ads, cookieKey + '=analytics'].join('; ')), id, 'all');
  assert.deepEqual(sends, [{...page, consent:analytics, ...ids}, {...page, consent:analytics, ...ids}]);
});

test('drops malformed, ambiguous, oversized and non-ASCII optional identifiers', async t => {
  const sends: unknown[] = [];
  t.mock.method(globalThis, 'fetch', async (_url: string, options: RequestInit) => {
    sends.push(JSON.parse(String(options.body))); return new Response(null, {status:204});
  });
  const session = '_ga_R4TD2Z8RR7=';
  const dirtyAds = 'anymd_attribution=' + encodeURIComponent(JSON.stringify({gclid:'a\nb', gbraid:'你好', wbraid:'x'.repeat(201), email:'private@example.invalid'}));
  const cases = [
    [ga, ga, session + 'GS2.1.s1700000001$s1700000002', dirtyAds, '_fbc=%GG', '_fbp=fb.1.1700000000000.%E4%BD%A0%E5%A5%BD'],
    ['_ga=GA1.1.12345', session + 'GS2.1.o2$h0', clicks, clicks, '_fbc=' + meta.fbc, '_fbc=' + meta.fbc, '_fbp=' + '1'.repeat(513)],
    ['_ga=GA1.1.123.456%0A', session + 'GS1.1.0.2', 'anymd_attribution=%7Bbroken', '_fbc=fb.1.1700000000000.Click%20Id'],
  ];
  for (const parts of cases) await bindCheckoutAttribution(money, headers(parts.join('; ')), id, 'all');
  assert.deepEqual(sends, cases.map(() => ({...page, consent:all})));
});

test('a detached binding has its own 750 ms abort and logs only fixed diagnostics', async t => {
  const warn = t.mock.method(console, 'warn', () => undefined);
  const deadline = t.mock.method(AbortSignal, 'timeout');
  const started = performance.now();
  const post = t.mock.method(globalThis, 'fetch', async (_url: string, options: RequestInit) => new Promise<Response>((_resolve, reject) => {
    options.signal!.addEventListener('abort', () => reject(new Error([money.pk, id, ga, 'private upstream body', money.origin].join(' '))));
  }));
  const hold = setTimeout(() => {}, 1500); // AbortSignal's timer may be unref'd.
  try {
    let finished = false;
    const pending = bindCheckoutAttribution(money, headers(ga), id, 'analytics').then(() => { finished = true; });
    assert.equal(post.mock.callCount(), 1);
    assert.deepEqual(deadline.mock.calls[0].arguments, [750]);
    assert.equal(finished, false);
    await pending;
    assert.ok(performance.now() - started >= 725);
    assert.deepEqual(warn.mock.calls.map(call => call.arguments), [['Checkout attribution binding unavailable']]);
  } finally { clearTimeout(hold); }
});

test('does not read the upstream body or expose identifiers on rejection or outage', async t => {
  const warn = t.mock.method(console, 'warn', () => undefined);
  let cancelled = false;
  const post = t.mock.method(globalThis, 'fetch', async () => new Response(new ReadableStream({
    start(controller) { controller.enqueue([money.pk, id, ga, money.origin]); },
    cancel() { cancelled = true; },
  }), {status:503}));
  await bindCheckoutAttribution(money, headers(ga), id, 'analytics');
  assert.equal(cancelled, true);
  post.mock.mockImplementation(async () => { throw new Error(money.pk + id + ga + money.origin); });
  await bindCheckoutAttribution(money, headers(ga), id, 'analytics');
  assert.deepEqual(warn.mock.calls.map(call => call.arguments), [
    ['Checkout attribution binding rejected', 503], ['Checkout attribution binding unavailable'],
  ]);
});
