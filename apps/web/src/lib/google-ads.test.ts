import assert from 'node:assert/strict';
import { test } from 'node:test';
import { resolveEffectiveConsent } from '@sylphx/sdk/signal';
import { fullAdConsent, googleAdsConfig, liveCheckoutSession } from './google-ads';

test('Ads config exposes a valid pair or neither value', () => {
  for (const id of ['AW-123456', 'AW-123456789012345']) {
    for (const label of ['k', 'X_1-z', 'X'.repeat(64)]) assert.deepEqual(googleAdsConfig(id, label), {googleAdsId:id, googleAdsPurchaseLabel:label});
  }
  for (const id of [undefined, '', 'AW-', 'AW-12345', 'AW-1234567890123456', 'G-123456', 'aw-123456', 'AW-123456\n', ' AW-123456']) assert.equal(googleAdsConfig(id, 'Label'), undefined);
  for (const label of [undefined, '', ' ', 'A'.repeat(65), 'a/b', '<script>', 'ok\n']) assert.equal(googleAdsConfig('AW-123456', label), undefined);
});

test('Ads requires both effective grants and no privacy override', () => {
  const all = resolveEffectiveConsent({storedChoice:'granted'});
  assert.equal(fullAdConsent(all), true);
  assert.equal(fullAdConsent(all, true), false);
  for (const consent of [undefined, resolveEffectiveConsent(), resolveEffectiveConsent({storedChoice:'denied'}),
    resolveEffectiveConsent({storedChoice:'analytics_only'}), resolveEffectiveConsent({storedChoice:'granted'}, new Headers({'Sec-GPC':'1'})),
    {...all, ad_user_data:'denied'} as const, {...all, ad_storage:'denied'} as const]) assert.equal(fullAdConsent(consent), false);
});

test('only bounded live processor IDs are eligible for a claim', () => {
  assert.equal(liveCheckoutSession('cs_live_' + 'A'.repeat(124)), true);
  for (const value of [1, undefined, 'cs_live_', 'cs_live_A\n', 'cs_live_a/a', 'cs_test_Yes', 'chk-live', 'cs_live_' + 'A'.repeat(125)]) assert.equal(liveCheckoutSession(value), false);
});
