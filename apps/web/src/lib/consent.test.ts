import assert from 'node:assert/strict';
import { test } from 'node:test';
import { consentFor, parseChoice, traceRegion } from './consent';

const host = 'anymd.sylphx.com';
const denied = {analytics_storage:'denied', ad_storage:'denied', ad_user_data:'denied', ad_personalization:'denied'};
const analytics = {...denied, analytics_storage:'granted'};
const full = {...analytics, ad_storage:'granted', ad_user_data:'granted'};
const trace = (country: string) => 'h=' + host + '\r\nloc=' + country + '\r\n';

test('EEA, UK, CH and unrecognized geography fail closed', () => {
  for (const code of 'AT BE BG HR CY CZ DK EE FI FR DE GR HU IS IE IT LV LI LT LU MT NL NO PL PT RO SK SI ES SE GB CH AX GF GP MQ MF RE YT XX T1 ZZ UK'.split(' ')) {
    assert.equal(traceRegion(trace(code), host), 'opt-in', code);
  }
  for (const response of ['', 'loc=US', trace('US') + 'loc=US', 'h=example.invalid\nloc=US', trace('us')]) {
    assert.equal(traceRegion(response, host), 'opt-in');
  }
  for (const code of ['US', 'CA', 'BR', 'JP', 'HK', 'AU']) assert.equal(traceRegion(trace(code), host), 'opt-out', code);
});

test('explicit choices win over defaults, and GPC overrides every choice', () => {
  assert.deepEqual(consentFor('opt-in', null, false), denied);
  assert.deepEqual(consentFor('opt-out', null, false), full);
  for (const region of ['opt-in', 'opt-out'] as const) {
    assert.deepEqual(consentFor(region, 'reject', false), denied);
    assert.deepEqual(consentFor(region, 'analytics', false), analytics);
    assert.deepEqual(consentFor(region, 'all', false), full);
    for (const choice of [null, 'reject', 'analytics', 'all'] as const) assert.deepEqual(consentFor(region, choice, true), denied);
  }
  assert.equal(parseChoice('granted'), null);
});
