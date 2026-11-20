import { expect, test } from 'bun:test';
import { currencyOptions, price, product, tokenProduct } from '../apps/web/src/lib/catalog';

const expected = {
  USD: '$29',
  HKD: 'HK$229',
  TWD: 'NT$959',
  GBP: '£21.99',
  EUR: '€26.99',
  JPY: '¥4,639',
  KRW: '₩38,900',
  MYR: 'RM118.99',
  SGD: 'S$37.99',
  CAD: 'CA$41.99',
  AUD: 'A$42.99',
  NZD: 'NZ$52.99',
  THB: '฿989',
  PHP: '₱1,859',
  IDR: 'Rp529,000',
  VND: '₫749,000',
};

test('existing once-off offer, currency labels and entitlement identity stay in sync', () => {
  expect(product.key).toBe('anymd_pro');
  expect(price.key).toBe('anymd_pro');
  expect(tokenProduct).toBe('anymd');
  expect(currencyOptions).toHaveLength(16);
  const labels = Object.fromEntries(currencyOptions.map(({ code, label }) => [code, label]));
  for (const [code, label] of Object.entries(expected)) expect(labels[code]).toBe(label);
  for (const { code, label } of currencyOptions) {
    expect(label).not.toContain('undefined');
    expect(Object.keys(price.unit_amounts)).toContain(code);
  }
});
