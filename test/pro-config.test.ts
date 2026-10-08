import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import config from '../docs/.vitepress/pro/config.json';
import {
  buyHref,
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

  test('the Buy button links to the anymd product checkout', () => {
    expect(config.buyUrl).toBe('https://buy.sylphx.com/buy/anymd');
    expect(isPlaceholder(config.buyUrl)).toBe(false);
    const button = readFileSync(
      new URL('../docs/.vitepress/theme/components/ProBuy.vue', import.meta.url),
      'utf8',
    );
    expect(button).toContain('const href = ref(buyUrl)');
    expect(button).not.toMatch(/buyMailto|buyReady|mailto:/);
    expect(button).toContain('US$29 once');
  });

  test('purchase copy promises instant on-page delivery, not an emailed token', () => {
    for (const path of ['../README.md', '../docs/pro.md', '../docs/pro/thanks.md']) {
      const copy = readFileSync(new URL(path, import.meta.url), 'utf8');
      expect(copy.toLowerCase()).toContain('instant');
      expect(copy).toContain('checkout confirmation page');
      expect(copy).not.toMatch(/token.*(?:arrives|on its way).*by email|within a few hours/);
    }
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

  test('the product checkout URL stays unchanged for a click id', () => {
    // Only direct Stripe payment links accept client_reference_id.
    expect(buyHref('abc123')).toBe(config.buyUrl);
    expect(buyHref('')).toBe(config.buyUrl);
  });
});
