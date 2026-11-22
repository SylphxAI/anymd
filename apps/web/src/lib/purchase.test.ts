import assert from 'node:assert/strict';
import { test } from 'node:test';
import { paidPurchase } from './purchase';

const expected = {name:'orgs/org_demo/projects/prj_demo/envs/env_demo/checkout_sessions/chk-000000000000000000000001', user:'usr_demo', price:'anymd_pro', processorSessionId:'cs_live_demo', live:true, currencyScale:100};
const paid = {name:expected.name, subject:{end_user:expected.user}, processor_session_id:expected.processorSessionId, line_items:[{price:expected.price, quantity:1}], state:'complete', payment_status:'paid', livemode:true, amount_total:'2900', currency_code:'USD'};

test('projects only a stable opaque transaction ID, charged currency and major-unit amount', () => {
  const result = paidPurchase({...paid, email:'fixture@example.invalid'}, expected)!;
  assert.deepEqual(Object.keys(result).sort(), ['currency', 'transaction_id', 'value']);
  assert.match(result.transaction_id, /^[a-f0-9]{64}$/);
  assert.equal(result.currency, 'USD');
  assert.equal(result.value, 29);
  assert.equal(paidPurchase(paid, expected)?.transaction_id, result.transaction_id);
  assert.equal(paidPurchase({...paid, currency_code:'JPY', amount_total:4639}, {...expected, currencyScale:1})?.value, 4639);
});

test('rejects wrong ownership, wrong checkout, unpaid, reversed, unrecognized, and unsafe amounts', () => {
  for (const patch of [{subject:{end_user:'usr_other'}}, {processor_session_id:'cs_live_other'}, {name:expected.name + '1'}, {payment_status:'unpaid'}, {state:'open'}, {payment_status:'revoked'}, {line_items:[{price:'other',quantity:1}]}, {livemode:false}, {amount_total:-1}, {amount_total:'Infinity'}, {currency_code:'USD,E'}]) {
    assert.equal(paidPurchase({...paid, ...patch}, expected), null);
  }
});
