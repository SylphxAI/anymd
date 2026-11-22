import assert from 'node:assert/strict';
import { test } from 'node:test';
import { explicitAnalytics } from './consent';
import { checkoutParameters, checkoutSurface } from './funnel';

test('conversion requires an explicit choice and GPC always wins', () => {
  assert.equal(explicitAnalytics(null, false), false);
  assert.equal(explicitAnalytics('reject', false), false);
  assert.equal(explicitAnalytics('analytics', false), true);
  assert.equal(explicitAnalytics('all', false), true);
  assert.equal(explicitAnalytics('analytics', true), false);
  assert.equal(explicitAnalytics('all', true), false);
});

test('only known public entry surfaces leave the browser', () => {
  assert.equal(checkoutSurface('/pricing','?entry_surface=studio_cite_preview'), 'studio_cite_preview');
  assert.equal(checkoutSurface('/pricing','?entry_surface=cli_pdf'), 'cli_pdf');
  assert.equal(checkoutSurface('/pricing','?entry_surface=cli_video'), 'cli_video');
  assert.equal(checkoutSurface('/pricing','?entry_surface=cli_unrecognized'), 'pricing');
  assert.equal(checkoutSurface('/pro','?entry_surface=seo_vs-marker&entry_surface=npm_citra'), 'pro');
  assert.equal(checkoutSurface('/pricing','?entry_surface=seo_private-investor-name&email=secret%40example.invalid'), 'pricing');
});

test('checkout uses the selected catalogue price and currency scale', () => {
  assert.deepEqual(checkoutParameters('USD','npm_citra'), {
    value:29, currency:'USD', plan_sku:'anymd_pro', entry_surface:'npm_citra',
    items:[{item_id:'anymd_pro', item_name:'Anymd Pro', quantity:1, price:29}],
  });
  assert.equal(checkoutParameters('JPY','pricing')?.value, 4639);
  assert.equal(checkoutParameters('toString','pricing'), undefined);
});
