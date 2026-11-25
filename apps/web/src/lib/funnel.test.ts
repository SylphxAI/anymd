import assert from 'node:assert/strict';
import { test } from 'node:test';
import { carryCampaign } from './campaign';
import { explicitAnalytics } from './consent';
import { checkoutParameters, checkoutSurface } from './funnel';

test('conversion requires an explicit choice and GPC always wins', () => {
  const headers = new Headers();
  assert.equal(explicitAnalytics(null, headers), false);
  assert.equal(explicitAnalytics('reject', headers), false);
  assert.equal(explicitAnalytics('analytics', headers), true);
  assert.equal(explicitAnalytics('all', headers), true);
  headers.set('Sec-GPC', '1');
  assert.equal(explicitAnalytics('analytics', headers), false);
  assert.equal(explicitAnalytics('all', headers), false);
});

test('only known public entry surfaces leave the browser', () => {
  assert.equal(checkoutSurface('/pricing','?entry_surface=studio_cite_preview'), 'studio_cite_preview');
  assert.equal(checkoutSurface('/pricing','?entry_surface=article_bridge&utm_source=docs'), 'article_bridge');
  assert.equal(checkoutSurface('/pricing','?entry_surface=cli_pdf'), 'cli_pdf');
  assert.equal(checkoutSurface('/pricing','?entry_surface=cli_video'), 'cli_video');
  assert.equal(checkoutSurface('/pricing','?entry_surface=cli_unrecognized'), 'pricing');
  assert.equal(checkoutSurface('/pro','?entry_surface=seo_vs-marker&entry_surface=npm_citra'), 'pro');
  assert.equal(checkoutSurface('/pricing','?entry_surface=seo_private-investor-name&email=secret%40example.invalid'), 'pricing');
});

for (const pathname of ['/pricing', '/zh-TW/pricing']) {
  test(pathname + ' keeps Pro CTA entry surfaces alongside campaign values', () => {
    for (const surface of ['home_hero', 'home_pro_panel', 'studio_quote_match']) {
      const href = carryCampaign(pathname + '?entry_surface=' + surface, '?gclid=TEST999&utm_source=docs&utm_campaign=pro');
      const url = new URL(href, 'https://anymd.invalid');
      assert.deepEqual(Object.fromEntries(url.searchParams), {
        entry_surface: surface, gclid: 'TEST999', utm_source: 'docs', utm_campaign: 'pro',
      });
      const resolved = checkoutSurface(url.pathname, url.search);
      assert.equal(resolved, surface);
      assert.equal(checkoutParameters('USD', resolved)?.entry_surface, surface);
    }
    assert.equal(checkoutSurface(pathname, '?entry_surface=home_unknown&utm_source=docs'), 'pricing');
    assert.equal(checkoutSurface(pathname, '?entry_surface=home_hero&entry_surface=home_pro_panel'), 'pricing');
  });
}

test('checkout uses the selected catalogue price and currency scale', () => {
  assert.deepEqual(checkoutParameters('USD','npm_citra'), {
    value:29, currency:'USD', plan_sku:'anymd_pro', entry_surface:'npm_citra',
    items:[{item_id:'anymd_pro', item_name:'Anymd Pro', quantity:1, price:29}],
  });
  assert.equal(checkoutParameters('JPY','pricing')?.value, 4639);
  assert.equal(checkoutParameters('toString','pricing'), undefined);
});
