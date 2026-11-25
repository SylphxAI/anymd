import assert from 'node:assert/strict';
import { test } from 'node:test';
import { consentKey } from './consent';
import { signalConfig } from './signal-config';
import { reportCheckoutStarted } from './signal-server';

const pk = ['sylphx', 'pk', 'fixture'].join('_');
const config = {SYLPHX_PUBLISHABLE_KEY:pk, SYLPHX_API_URL:'https://signal.example.invalid/'};
const checkout = 'chk-111111111111111111111111';

test('Signal config uses only a platform publishable key and an HTTPS origin', () => {
  assert.deepEqual(signalConfig(config), {apiKey:pk, baseUrl:'https://signal.example.invalid'});
  assert.deepEqual(signalConfig({SYLPHX_PUBLISHABLE_KEY:pk}), {apiKey:pk, baseUrl:'https://api.sylphx.com'});
  for (const base of ['http://127.0.0.1:3000', 'https://signal.example.invalid/v1', 'https://signal.example.invalid/?',
    'https://signal.example.invalid/#', 'https://user@signal.example.invalid', 'not a URL']) {
    assert.equal(signalConfig({...config, SYLPHX_API_URL:base}), undefined, base);
  }
  assert.equal(signalConfig({SYLPHX_API_URL:config.SYLPHX_API_URL}), undefined);
  assert.equal(signalConfig({...config, SYLPHX_PUBLISHABLE_KEY:['sylphx', 'sk', 'fixture'].join('_')}), undefined);
});

test('checkout sends a deterministic private-safe batch only on explicit consent, and failures are absorbed', async t => {
  const original = {...process.env};
  Object.assign(process.env, config);
  t.after(() => {
    for (const key of Object.keys(config)) {
      if (original[key] === undefined) delete process.env[key];
      else process.env[key] = original[key];
    }
  });
  const timeout = t.mock.method(AbortSignal, 'timeout');
  const requests: {url:string; init:RequestInit}[] = [];
  const send: typeof fetch = async (url, init) => {
    requests.push({url:String(url), init:init ?? {}});
    if (requests.length === 3) throw new Error('synthetic network failure');
    return new Response('fixture failure', {status:503});
  };
  const request = (cookie?: string, gpc?: string) => new Request('https://anymd.sylphx.com/api/anymd/checkout?email=private', {
    headers:{...(cookie ? {Cookie:cookie} : {}), ...(gpc === undefined ? {} : {'Sec-GPC':gpc})},
  });
  const analytics = consentKey + '=analytics';
  for (const [incoming, grant] of [[request(), true], [request(analytics), false], [request(analytics), 'true'],
    [request(analytics, '1'), true], [request(analytics, '0'), true], [request(consentKey + '=reject'), true],
    [request(analytics + '; ' + consentKey + '=all'), true]] as const) {
    await reportCheckoutStarted(incoming, grant, checkout, 'USD', 'pricing', send);
  }
  await reportCheckoutStarted(request(analytics), true, 'unchecked-session', 'USD', 'pricing', send);
  assert.equal(requests.length, 0);
  await reportCheckoutStarted(request(analytics), true, checkout, 'USD', 'pricing', send);
  await reportCheckoutStarted(request(consentKey + '=all'), true, checkout, 'USD', 'pricing', send);
  await reportCheckoutStarted(request(analytics), true, 'chk-222222222222222222222222', 'USD', 'pricing', send);
  assert.equal(requests.length, 3);
  const {url, init} = requests[0];
  assert.equal(url, 'https://signal.example.invalid/v1/analytics/events:batch');
  assert.equal(init.method, 'POST');
  assert.equal(init.redirect, 'error');
  assert.equal(init.cache, 'no-store');
  assert.deepEqual(init.headers, {'X-Sylphx-Key':pk, 'Content-Type':'application/json'});
  assert.ok(init.signal instanceof AbortSignal);
  assert.deepEqual(timeout.mock.calls.map(call => call.arguments), [[10_000], [10_000], [10_000]]);
  const body = JSON.parse(String(init.body));
  const id = body.events[0].event_id;
  assert.match(id, /^[a-f0-9]{64}$/);
  assert.deepEqual(body, {consent:'granted', events:[{event_id:id, distinct_id:id, event_name:'checkout_started',
    numeric_value:29, properties:{currency:'USD', plan_sku:'anymd_pro', entry_surface:'pricing'}}]});
  assert.equal(requests[1].init.body, init.body);
  assert.notEqual(JSON.parse(String(requests[2].init.body)).events[0].event_id, id);
});
