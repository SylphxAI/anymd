import assert from 'node:assert/strict';
import { test } from 'node:test';
import { bindCheckoutAttribution, detectSignalRegion, resolveEffectiveConsent } from '@sylphx/sdk/signal';
import { savedSignalChoice } from './consent';

const denied = {analytics_storage:'denied', ad_storage:'denied', ad_user_data:'denied', ad_personalization:'denied'};
const analytics = {...denied, analytics_storage:'granted'};
const full = {analytics_storage:'granted', ad_storage:'granted', ad_user_data:'granted', ad_personalization:'granted'};
const checkoutSessionId = 'chk-111111111111111111111111';
const identity = {checkout_session_id:checkoutSessionId};
const ga = {ga_client_id:'123.456', ga_session_id:'1700000001', page_location:'https://example.invalid/pricing?utm_source=search'};
const ads = {gclid:'Click_123', gbraid:'Braided_1', wbraid:'Braided_2', fbc:'fb.1.1700000000000.Other_Click', fbp:'fb.1.1700000000000.12345'};

async function binding(privacy: Parameters<typeof resolveEffectiveConsent>[0], gpc?: string) {
  const headers = new Headers({Cookie:'_ga=GA1.1.123.456; _ga_OTHER=GS1.1.999; _ga_EXAMPLE=GS2.1.s1700000001$o1; _fbc=' + ads.fbc + '; _fbp=' + ads.fbp});
  if (gpc) headers.set('Sec-GPC', gpc);
  let payload: unknown;
  await bindCheckoutAttribution({checkoutSessionId}, {
    apiKey:'fixture', headers, privacy, gaMeasurementId:'G-EXAMPLE',
    pageLocation:new URL('https://example.invalid/pricing?checkout_proof=private#private'),
    attribution:{gclid:ads.gclid, gbraid:ads.gbraid, wbraid:ads.wbraid, utm_source:'search'},
    fetch:async (url, options) => {
      assert.equal(String(url), 'https://api.sylphx.com/v1/events/attribution_bindings');
      payload = JSON.parse(String(options?.body));
      return Response.json({binding_id:'fixture', checkout_session_id:checkoutSessionId, processor_session_id:null, expires_at:'2030-01-01T00:00:00Z'}, {status:201});
    },
  });
  return payload;
}

test('US, CA and AU implicit grants send all four permissions and complete checkout attribution', async () => {
  for (const region of ['US','CA','AU']) {
    assert.deepEqual(resolveEffectiveConsent({region}), full);
    assert.deepEqual(await binding({region}), {...identity, consent:full, ...ga, ...ads});
  }
});

test('EEA, UK, Switzerland, and unknown regions never send tracking fields without opt-in', async () => {
  for (const region of ['EU','EEA','DE','UK','GB','CH','XX','T1',undefined]) {
    assert.deepEqual(resolveEffectiveConsent({region}), denied);
    assert.deepEqual(await binding({region}), {...identity, consent:denied});
  }
  for (const trace of ['', 'loc=US\nloc=CA\n', 'loc=XX\n']) {
    const region = await detectSignalRegion(async () => new Response(trace));
    assert.equal(region, undefined);
    assert.deepEqual(await binding({region}), {...identity, consent:denied});
  }
});

test('explicit choices survive migration; GPC overrides opt-in and opt-out defaults', async () => {
  assert.deepEqual(await binding({region:'US', storedChoice:savedSignalChoice('reject')}), {...identity, consent:denied});
  assert.deepEqual(await binding({region:'DE', storedChoice:savedSignalChoice('analytics')}), {...identity, consent:analytics, ...ga});
  assert.deepEqual(await binding({region:'CH', storedChoice:savedSignalChoice('all')}), {...identity, consent:full, ...ga, ...ads});
  for (const storedChoice of [undefined,'denied','analytics_only','granted'] as const) {
    assert.deepEqual(await binding({region:'US', storedChoice}, '0, 1'), {...identity, consent:denied});
  }
});
