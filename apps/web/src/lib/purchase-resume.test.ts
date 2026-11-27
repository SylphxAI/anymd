import assert from 'node:assert/strict';
import { test } from 'node:test';
import { pricingReturnPath, signInReturnPath } from './marketing-locale';
import { purchaseResume } from './purchase-resume';

test('describes the pricing return with the catalogue price', () => {
  assert.deepEqual(purchaseResume('/pricing?entry_surface=home_hero&currency=HKD', 'en'), {
    currency: 'HKD', price: 'HK$229', path: '/pricing', query: 'entry_surface=home_hero&currency=HKD',
  });
  assert.deepEqual(purchaseResume('/zh-TW/pricing?entry_surface=pricing&currency=TWD', 'zh-TW'), {
    currency: 'TWD', price: 'NT$959', path: '/zh-TW/pricing', query: 'entry_surface=pricing&currency=TWD',
  });
  assert.deepEqual(purchaseResume('/pricing', 'en'), { currency: 'USD', price: '$29', path: '/pricing', query: '' });
  assert.equal(purchaseResume('/pricing?currency=JPY', 'en')?.price, '¥4,639');
});

test('the sign-in page summarizes exactly what the pricing Buy link carries', () => {
  for (const [path, search, currency] of [
    ['/pricing', '?entry_surface=seo_vs-docling&currency=EUR&email=private', 'EUR'],
    ['/zh-TW/pricing', '?entry_surface=home_hero&currency=HKD', 'HKD'],
    ['/pricing', '?entry_surface=injected_bad&currency=ZZZ', 'USD'],
    ['/pro', '', 'USD'],
  ] as const) {
    const next = signInReturnPath('/sign-in', '?next=' + encodeURIComponent(pricingReturnPath(path, search)));
    assert.equal(purchaseResume(next, 'en')?.currency, currency, path + search);
  }
});

test('shows no purchase for other, unsafe or ambiguous return targets', () => {
  for (const next of [
    '/dashboard', '/pro', '/pricing/', '/pricingx', '/recover?session_id=cs_fixture', '/cli-auth?code=fixture',
    '//example.invalid/pricing', 'https://example.invalid/pricing', '/api/anymd/checkout', '/sign-in?next=/pricing',
    '/pricing?currency=ZZZ', '/pricing?currency=usd', '/pricing?currency=USD&currency=HKD', '/pricing?entry_surface=injected',
    '/pricing?amount=1&currency=USD', '/pricing?price=1', '/pricing?currency=HKD&session_id=cs_fixture', '/pricing?checkout=cancelled',
    '/pricing?currency=H%4BD', '/pricing?entry%5Fsurface=pricing', '/pricing#plans', '/zh-Hant/pricing', '/zh-TW/studio',
  ]) assert.equal(purchaseResume(next, 'en'), undefined, next);
});
