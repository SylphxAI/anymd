import assert from 'node:assert/strict';
// @ts-expect-error The Bun test loader is not part of the web app's Node types.
import { mock } from 'bun:test';
import { createHash } from 'node:crypto';
import { test } from 'node:test';
import { consentKey } from './consent';

// The root test suite runs before the web build generates the production binding.
mock.module('./money-production.generated', () => ({productionMoney:{
  origin:'https://production.example.invalid', parent:'orgs/org_prod/projects/prj_prod/envs/env_prod', pk:'unused',
}}));

const origin = 'http://localhost:3000';
const auth = 'https://auth.example.invalid';
const money = 'https://money.example.invalid';
const signal = 'https://signal.example.invalid';
const parent = 'orgs/org_fixture/projects/prj_fixture/envs/env_fixture';
const user = 'usr_fixture001';
const intent = 'a'.repeat(64);
const settings = {
  ANYMD_APP_ORIGIN:origin, ANYMD_MONEY_TEST_PARENT:parent, ANYMD_MONEY_TEST_URL:money, ANYMD_MONEY_MODE:undefined,
  ANYMD_MONEY_TEST_KEY:['sylphx', 'pk', 'test', 'fixture'].join('_'), ANYMD_MONEY_TEST_API_KEY:['sylphx', 'sk', 'test', 'fixture'].join('_'),
  SYLPHX_AUTH_URL:auth, SYLPHX_API_URL:signal, SYLPHX_AUTH_ORGANIZATION_ID:'organization-00000000-0000-0000-0000-000000000000',
  SYLPHX_AUTH_SECRET_KEY:['sylphx', 'sk', 'fixture'].join('_'), SYLPHX_PUBLISHABLE_KEY:['sylphx', 'pk', 'fixture'].join('_'),
};
const session = 'anymd_session=identity_org_session_fixture_legacy';
const optedIn = session + '; ' + consentKey + '=analytics';

function request(data: Record<string, unknown> = {}, headers: Record<string, string> = {}) {
  return new Request(origin + '/api/anymd/checkout', {
    method:'POST', headers:{Origin:origin, Host:'localhost:3000', 'Content-Type':'application/json', Cookie:optedIn, ...headers},
    body:JSON.stringify({currency:'USD', consent:true, checkoutIntent:intent, entrySurface:'pro', analyticsGranted:true, ...data}),
  });
}

test('only a Money refusal of an authorized checkout create sends an optional checkout_failed event', async t => {
  const {POST} = await import('../routes/api/anymd/[action]');
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
  const authRead = auth + '/v1/sessions/current';
  const create = money + '/v1/' + parent + '/checkout_sessions';
  const ingest = signal + '/v1/analytics/events:batch';
  let moneyReply: () => Response | Promise<Response> = () => Response.json({code:'PERMISSION_DENIED'}, {status:403});
  let signalReply: () => Response | Promise<Response> = () => new Response(null, {status:204});
  const calls: string[] = [];
  const batches: {raw: string; body: {events: {event_name: string}[]}}[] = [];
  globalThis.fetch = (async (url, init) => {
    const outgoing = new Request(url, init);
    calls.push(outgoing.url);
    if (outgoing.url === authRead && outgoing.headers.get('authorization') === 'Bearer identity_org_session_fixture_legacy') {
      return Response.json({session:{principal:{project_id:settings.SYLPHX_AUTH_ORGANIZATION_ID, principal_id:user, state:'active'}}});
    }
    if (outgoing.url === create && outgoing.method === 'POST' &&
        outgoing.headers.get('authorization') === 'Bearer ' + settings.ANYMD_MONEY_TEST_API_KEY) {
      assert.equal(outgoing.headers.get('idempotency-key'), `anymd-${user}-${intent}`);
      return moneyReply();
    }
    if (outgoing.url === ingest && outgoing.method === 'POST') {
      const raw = await outgoing.text();
      batches.push({raw, body:JSON.parse(raw)});
      return signalReply();
    }
    throw new Error('unexpected outbound request: ' + outgoing.url);
  }) as typeof fetch;
  // The optional send is not awaited by checkout; let it drain before reading the fixture.
  const drain = () => new Promise(resolve => setTimeout(resolve, 20));
  const submit = async (req: Request, status: number, body: unknown) => {
    calls.length = 0;
    batches.length = 0;
    const response = await POST({request:req, params:{action:'checkout'}} as never);
    await drain();
    assert.equal(response.status, status);
    assert.deepEqual(await response.json(), body);
    return batches.map(batch => batch.body);
  };
  const failed = (status: string) => {
    const event_id = createHash('sha256').update('anymd:checkout_failed:' + intent).digest('hex');
    return {consent:'granted', events:[{event_id, event_name:'checkout_failed', distinct_id:event_id,
      properties:{status, entry_surface:'pro'}}]};
  };

  // Positive: Money answered and refused the create call. The refusal reaches the buyer unchanged.
  assert.deepEqual(await submit(request(), 403, {code:'billing_refused'}), [failed('refused')]);
  for (const forbidden of [user, intent, 'checkout_sessions', 'http', 'PERMISSION_DENIED', 'USD', 'numeric_value']) {
    assert.ok(!batches[0].raw.includes(forbidden), forbidden);
  }
  // A retry with the same intent produces the same event ID, so Signal keeps one record.
  assert.deepEqual(await submit(request(), 403, {code:'billing_refused'}), [failed('refused')]);
  moneyReply = () => Response.json({code:'RESOURCE_EXHAUSTED'}, {status:429});
  assert.deepEqual(await submit(request(), 429, {code:'rate_limited'}), [failed('rate_limited')]);
  moneyReply = () => Response.json({code:'INVALID_ARGUMENT'}, {status:400});
  assert.deepEqual(await submit(request(), 400, {code:'billing_unavailable'}), [failed('rejected')]);

  // Signal failure cannot change the refusal.
  signalReply = () => { throw new Error('fixture transport failure'); };
  assert.deepEqual(await submit(request(), 400, {code:'billing_unavailable'}), [failed('rejected')]);
  signalReply = () => new Response('unavailable', {status:503});
  assert.deepEqual(await submit(request(), 400, {code:'billing_unavailable'}), [failed('rejected')]);
  signalReply = () => new Response(null, {status:204});

  // No opt-in, a rejected or duplicate choice, a missing browser grant and GPC send nothing.
  moneyReply = () => Response.json({code:'PERMISSION_DENIED'}, {status:403});
  for (const req of [request({}, {Cookie:session}), request({}, {Cookie:session + '; ' + consentKey + '=reject'}),
    request({}, {Cookie:optedIn + '; ' + consentKey + '=all'}), request({analyticsGranted:false}),
    request({analyticsGranted:'true'}), request({}, {'Sec-GPC':'1'})]) {
    assert.deepEqual(await submit(req, 403, {code:'billing_refused'}), []);
    assert.ok(calls.includes(create));
  }

  // Invalid input and a signed-out buyer never reach Money, so nothing is reported.
  for (const data of [{currency:'ZZZ'}, {consent:false}, {checkoutIntent:'not-an-intent'}]) {
    assert.deepEqual(await submit(request(data), 400, {code:'invalid_request'}), []);
    assert.ok(!calls.includes(create));
  }
  assert.deepEqual(await submit(request({}, {Cookie:consentKey + '=analytics'}), 401, {code:'unauthorized'}), []);
  assert.ok(!calls.includes(create));

  // Unknown outcomes: Money may still hold a checkout for this key, so nothing is inferred.
  for (const [reply, status, code] of [
    [() => Response.json({}, {status:500}), 502, 'billing_unavailable'],
    [() => Response.json({}, {status:503}), 502, 'billing_unavailable'],
    [() => Response.json({code:'ABORTED'}, {status:409}), 409, 'billing_unavailable'],
    [() => Response.json({}, {status:408}), 408, 'billing_unavailable'],
    [() => new Response('not json', {status:200}), 502, 'billing_unavailable'],
    [() => { throw new TypeError('fixture network failure'); }, 502, 'upstream_unavailable'],
    [() => { throw new DOMException('fixture timeout', 'TimeoutError'); }, 502, 'upstream_unavailable'],
  ] as const) {
    moneyReply = reply;
    assert.deepEqual(await submit(request(), status, {code}), [], `${status} ${code}`);
  }

  // A created checkout is never a failure, even if its link is unusable afterwards.
  const name = parent + '/checkout_sessions/chk-111111111111111111111111';
  moneyReply = () => Response.json({name, url:'https://checkout.stripe.com/c/pay/fixture'});
  const created = await submit(request(), 200, {url:'https://checkout.stripe.com/c/pay/fixture', attribution:{
    checkoutSessionId:'chk-111111111111111111111111', apiKey:settings.SYLPHX_PUBLISHABLE_KEY, baseUrl:signal, gpc:false}});
  assert.deepEqual(created.map(batch => batch.events[0].event_name), ['checkout_started']);
  moneyReply = () => Response.json({name, url:'https://processor.example.invalid/pay'});
  assert.deepEqual(await submit(request(), 502, {code:'billing_unavailable'}), []);
});
