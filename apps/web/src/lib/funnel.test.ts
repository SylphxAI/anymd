import assert from 'node:assert/strict';
import { test } from 'node:test';
import { carryCampaign } from './campaign';
import { explicitAnalytics } from './consent';
import { checkoutParameters, checkoutSurface, proPricingTarget } from './funnel';
import { signupSignal } from './signup';

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

test('the /pro alias keeps checkout and sign-up attribution on /pricing', () => {
  const searches = ['', '?currency=HKD&utm_source=x', '?entry_surface=cli_pdf', '?utm_source=docs+nav&entry_surface=npm_citra&gclid=Ab%2bc',
    '?entry_surface=pro', '?entry_surface=pricing', '?entry_surface=private@example.invalid', '?entry_surface=cli_unrecognized&currency=EUR',
    '?entry_surface=seo_vs-marker&entry_surface=npm_citra', '?entry_surface=pro&entry_surface=pro', '?entry_surface=', '?entry_surface',
    '?entry%5Fsurface=npm_citra&entry_surface=cli_pdf', '?tag=one&tag=two&next=https%3A%2F%2Fexample.invalid', '?a=1&&b=2'];
  for (const search of searches) {
    const target = proPricingTarget(search);
    const url = new URL(target, 'https://anymd.sylphx.com');
    assert.equal(url.pathname, '/pricing', search);
    assert.ok(url.searchParams.getAll('entry_surface').length === 1, search);
    assert.equal(checkoutSurface(url.pathname, url.search), checkoutSurface('/pro', search), search);
    assert.deepEqual(signupSignal(true, 'analytics', undefined, 'google', target), signupSignal(true, 'analytics', undefined, 'google', '/pro' + search), search);
  }
  assert.equal(proPricingTarget(''), '/pricing?entry_surface=pro');
  assert.equal(proPricingTarget('?currency=HKD&utm_source=x'), '/pricing?currency=HKD&utm_source=x&entry_surface=pro');
  assert.equal(proPricingTarget('?entry_surface=cli_pdf&currency=EUR'), '/pricing?entry_surface=cli_pdf&currency=EUR');
  assert.equal(proPricingTarget('?entry_surface=private@example.invalid&utm_source=x'), '/pricing?utm_source=x&entry_surface=pro');
  assert.equal(proPricingTarget('?entry_surface=seo_vs-marker&gclid=Ab%2bc&entry_surface=npm_citra'), '/pricing?gclid=Ab%2bc&entry_surface=pro');
});
