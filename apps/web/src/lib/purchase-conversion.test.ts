import assert from 'node:assert/strict';
// @ts-expect-error The Bun test loader is not part of the web app's Node types.
import { mock } from 'bun:test';
import { createHash } from 'node:crypto';
import { test } from 'node:test';
import { consentKey } from './consent';

// SolidStart resolves this build-time marker; it has no runtime behavior.
mock.module('server-only', () => ({}));

const origin = 'http://localhost:3000';
const auth = 'https://auth.example.invalid';
const money = 'https://money.example.invalid';
const signal = 'https://signal.example.invalid';
const parent = 'orgs/org_fixture/projects/prj_fixture/envs/env_fixture';
const checkoutName = parent + '/checkout_sessions/chk-000000000000000000000001';
const sid = 'cs_live_FixturePaid';
const subject = {end_user:'usr_fixture001'};
const verified = {name:checkoutName, processor_session_id:sid, subject, livemode:true, state:'complete', payment_status:'paid',
  amount_total:'2999', currency_code:'USD', line_items:[{price:'anymd_pro', quantity:1}], metadata:{entry_surface:'pro', ref:'private'},
  customer_email:'test@example.invalid'};
const settings = {
  ANYMD_APP_ORIGIN:origin, ANYMD_MONEY_TEST_PARENT:parent, ANYMD_MONEY_TEST_URL:money,
  ANYMD_MONEY_TEST_KEY:['sylphx', 'pk', 'test', 'fixture'].join('_'), ANYMD_MONEY_TEST_API_KEY:['sylphx', 'sk', 'test', 'fixture'].join('_'),
  SYLPHX_AUTH_URL:auth, SYLPHX_API_URL:signal, SYLPHX_AUTH_ORGANIZATION_ID:'organization-00000000-0000-0000-0000-000000000000',
  SYLPHX_AUTH_SECRET_KEY:['sylphx', 'sk', 'fixture'].join('_'), SYLPHX_PUBLISHABLE_KEY:['sylphx', 'pk', 'fixture'].join('_'),
  ANYMD_GOOGLE_ADS_ID:undefined, ANYMD_GOOGLE_ADS_PURCHASE_LABEL:undefined,
};

function request(data: Record<string, unknown> = {}, headers: Record<string, string> = {}) {
  return new Request(origin + '/api/anymd/purchase-conversion', {
    method:'POST', headers:{Origin:origin, Host:'localhost:3000', 'Content-Type':'application/json',
      Cookie:'anymd_session=identity_org_session_fixture_legacy; ' + consentKey + '=analytics', ...headers},
    body:JSON.stringify({session_id:sid, analytics_consent:true, numeric_value:999, currency:'ZZZ', ...data}),
  });
}

test('purchase return verifies Money once and independently admits Signal and Ads', async t => {
  const {purchaseConversion} = await import('./purchase-conversion');
  const original = {...process.env};
  for (const [key, value] of Object.entries(settings)) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
  const originalFetch = globalThis.fetch;
  t.after(() => {
    for (const key of Object.keys(settings)) {
      if (original[key] === undefined) delete process.env[key];
      else process.env[key] = original[key];
    }
    globalThis.fetch = originalFetch;
  });
  let row = verified;
  let signalFails = false;
  const calls: Request[] = [];
  const batches: unknown[] = [];
  const authRead = auth + '/v1/sessions/current';
  const redeem = money + '/v1/money/checkout_sessions:redeem';
  const read = money + '/v1/' + checkoutName;
  const ingest = signal + '/v1/analytics/events:batch';
  globalThis.fetch = (async (url, init) => {
    const outgoing = new Request(url, init);
    calls.push(outgoing);
    if (outgoing.url === authRead && outgoing.method === 'GET' &&
        outgoing.headers.get('authorization') === 'Bearer ' + 'identity_org_session_fixture_legacy') {
      return Response.json({session:{principal:{project_id:settings.SYLPHX_AUTH_ORGANIZATION_ID, principal_id:subject.end_user, state:'active'}}});
    }
    if (outgoing.url === redeem && outgoing.method === 'POST' &&
        outgoing.headers.get('authorization') === 'Bearer ' + settings.ANYMD_MONEY_TEST_KEY) {
      assert.deepEqual(await outgoing.json(), {checkout_session:sid});
      return Response.json({checkout_session:checkoutName, paid:true});
    }
    if (outgoing.url === read && outgoing.method === 'GET' &&
        outgoing.headers.get('authorization') === 'Bearer ' + settings.ANYMD_MONEY_TEST_API_KEY) return Response.json(row);
    if (outgoing.url === ingest && outgoing.method === 'POST') {
      assert.deepEqual([...outgoing.headers], [['content-type','application/json'], ['x-sylphx-key',settings.SYLPHX_PUBLISHABLE_KEY]]);
      assert.equal(outgoing.redirect, 'error');
      batches.push(await outgoing.json());
      if (signalFails) throw new Error('fixture transport failure');
      return new Response(null, {status:204});
    }
    throw new Error('unexpected outbound request: ' + outgoing.url);
  }) as typeof fetch;
  const submit = async (status: number, outgoing: string[], req = request()) => {
    calls.length = 0;
    const result = await purchaseConversion(req);
    assert.equal(result.status, status);
    assert.equal(result.headers.get('cache-control'), 'private, no-store');
    assert.deepEqual(calls.map(call => call.url), outgoing);
    if (status !== 200) assert.equal(result.headers.get('set-cookie'), null);
    return result;
  };
  const verifiedCalls = [authRead, redeem, read];
  const measuredCalls = [...verifiedCalls, ingest];
  await submit(204, measuredCalls);
  await submit(204, measuredCalls, request({ads_consent:true}, {Cookie:'anymd_session=identity_org_session_fixture_legacy; ' + consentKey + '=all'}));
  const eventId = createHash('sha256').update('anymd:purchase_completed:' + checkoutName).digest('hex');
  const expected = {consent:'granted', events:[{event_id:eventId, distinct_id:eventId, event_name:'purchase_completed',
    numeric_value:29.99, properties:{currency:'USD', plan_sku:'anymd_pro', entry_surface:'pro'}}]};
  assert.deepEqual(batches, [expected, expected]);

  for (const req of [request({}, {'Sec-GPC':'1'}), request({ads_consent:true}, {'Sec-GPC':'1'}), request({analytics_consent:'true'}),
    request({}, {Cookie:consentKey + '=reject'}), request({}, {Cookie:consentKey + '=analytics; ' + consentKey + '=all'}),
    request({}, {Cookie:''}), request({}, {Origin:'https://other.example.invalid'})]) await submit(403, [], req);
  await submit(401, [], request({}, {Cookie:consentKey + '=analytics'}));
  await submit(204, [], request({session_id:'cs_test_FixturePaid'}));
  for (const props of [{payment_status:'unpaid'}, {state:'open'}, {denied_reason:'country_denied'}, {livemode:false}, {livemode:undefined},
    {processor_session_id:'cs_live_Other'}, {subject:{end_user:'usr_other001'}}, {amount_total:'0'}, {currency_code:'ZZZ'},
    {line_items:[{price:'unrelated_product'}]}]) {
    row = {...verified, ...props} as typeof verified;
    await submit(204, verifiedCalls);
  }
  assert.equal(batches.length, 2);
  row = {...verified, amount_total:'4639', currency_code:'JPY', metadata:{entry_surface:'test@example.invalid', ref:'private'}};
  await submit(204, measuredCalls);
  assert.deepEqual(batches[2], {consent:'granted', events:[{event_id:eventId, distinct_id:eventId, event_name:'purchase_completed',
    numeric_value:4639, properties:{currency:'JPY', plan_sku:'anymd_pro'}}]});
  row = verified;

  process.env.ANYMD_GOOGLE_ADS_ID = 'AW-123456789';
  process.env.ANYMD_GOOGLE_ADS_PURCHASE_LABEL = 'Fixture_Label';
  await submit(204, measuredCalls); // Analytics only even though Ads is configured.
  const all = 'anymd_session=identity_org_session_fixture_legacy; ' + consentKey + '=all';
  signalFails = true;
  const claim = await submit(200, measuredCalls, request({ads_consent:true}, {Cookie:all}));
  const transactionId = createHash('sha256').update('anymd-ads-purchase:' + checkoutName).digest('hex').slice(0, 32);
  assert.deepEqual(await claim.json(), {transactionId, value:29.99, currency:'USD'});
  const cookie = claim.headers.get('set-cookie') ?? '';
  assert.match(cookie, /^__Secure-anymd_ads_claims=[a-f0-9]{32}~[a-f0-9]{64}; Path=\/api\/anymd\/purchase-conversion;.*HttpOnly; Secure; SameSite=Lax$/);
  signalFails = false;
  await submit(204, measuredCalls, request({ads_consent:true}, {Cookie:all + '; ' + cookie.split(';')[0]}));
  assert.deepEqual(batches.slice(3), [expected, expected, expected]);
});
