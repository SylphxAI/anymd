import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readSettledCheckout } from './money-http';

const money = {origin:'https://money.example.invalid', parent:'orgs/org_test/projects/prj_test/envs/env_test', pk:'publishable-fixture'};
const name = money.parent + '/checkout_sessions/chk_good';
const sid = 'cs_live_fixture';

test('checkout readback admits only exact 200s and the same tenant resource', async () => {
  const previous = fetch;
  const calls: Request[] = [];
  try {
    const receipt = {paid:true, checkout_session:name};
    for (const [status, result] of [[201, receipt], [200, {...receipt, paid:false}], [200, {...receipt, checkout_session:name + '/../other'}],
      [200, {...receipt, checkout_session:'other/checkout_sessions/chk_good'}]] as const) {
      calls.length = 0;
      globalThis.fetch = (async (url, init) => { calls.push(new Request(url, init)); return Response.json(result, {status}); }) as typeof fetch;
      assert.equal(await readSettledCheckout(money, sid, 'read-fixture'), undefined);
      assert.equal(calls.length, 1);
    }
    for (const [status, row, accepted] of [[201, {name}, false], [200, {name:name + 'other'}, false], [200, {name}, true]] as const) {
      calls.length = 0;
      globalThis.fetch = (async (url, init) => {
        calls.push(new Request(url, init));
        return calls.length === 1 ? Response.json(receipt) : Response.json(row, {status});
      }) as typeof fetch;
      assert.deepEqual(await readSettledCheckout(money, sid, 'read-fixture'), accepted ? row : undefined);
      assert.deepEqual(calls.map(r => [r.method, r.url, r.headers.get('Authorization')]), [
        ['POST', money.origin + '/v1/money/checkout_sessions:redeem', 'Bearer ' + money.pk],
        ['GET', money.origin + '/v1/' + name, 'Bearer read-fixture'],
      ]);
      assert.deepEqual(await calls[0].json(), {checkout_session:sid});
    }
  } finally { globalThis.fetch = previous; }
});
