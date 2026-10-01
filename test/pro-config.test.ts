import { describe, expect, test } from 'bun:test';
import config from '../docs/.vitepress/pro/config.json';
import {
  buyHref,
  buyMailto,
  buyReady,
  isPlaceholder,
  trackPurchase,
  transactionId,
} from '../docs/.vitepress/pro/tracking';

describe('pro config', () => {
  test('placeholders are recognised, including the payment link placeholder', () => {
    expect(isPlaceholder('https://PRO_PAYMENT_LINK')).toBe(true);
    expect(isPlaceholder('G-XXXX')).toBe(true);
    expect(isPlaceholder('')).toBe(true);
    expect(isPlaceholder('https://buy.stripe.com/abc')).toBe(false);
  });

  test('the fallback is a purchase email, not a dead link', () => {
    expect(buyMailto).toBe('mailto:hi@sylphx.com?subject=anymd%20Pro%20purchase');
  });

  test('the Buy button never links to a placeholder (it falls back to the purchase email)', () => {
    const rendered = buyReady ? config.buyUrl : buyMailto;
    expect(isPlaceholder(rendered)).toBe(false);
  });
});

describe('purchase attribution', () => {
  test('only a Stripe session id counts as a purchase', () => {
    expect(transactionId('?session_id=cs_test_a1B2c3')).toBe('cs_test_a1B2c3');
    expect(transactionId('')).toBe('');
    expect(transactionId('?session_id=abc')).toBe('');
    expect(transactionId('?session_id=cs_bad-id')).toBe('');
    expect(trackPurchase('')).toBe('');
  });

  test('the click id is appended only to Stripe links, never to a mailto or placeholder', () => {
    // config.buyUrl is a placeholder here, so buyHref must return it untouched.
    expect(buyHref('abc123')).toBe(config.buyUrl);
    expect(buyHref('')).toBe(config.buyUrl);
  });
});
