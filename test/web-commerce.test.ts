import { expect, spyOn, test } from 'bun:test';
import {
  currencies,
  formatPrice,
  isCurrency,
  price,
  proAmount,
  product,
  tokenProduct,
} from '../apps/web/src/lib/catalog';

import { failure } from '../apps/web/src/lib/http';
import { moneyCall } from '../apps/web/src/lib/money-http';

const expected = {
  USD: 29,
  HKD: 229,
  TWD: 959,
  GBP: 21.99,
  EUR: 26.99,
  JPY: 4639,
  KRW: 38900,
  MYR: 118.99,
  SGD: 37.99,
  CAD: 41.99,
  AUD: 42.99,
  NZD: 52.99,
  THB: 989,
  PHP: 1859,
  IDR: 529000,
  VND: 749000,
};

test('the once-off prices and entitlement identity are unchanged', () => {
  expect(product.key).toBe('anymd_pro');
  expect(price.key).toBe('anymd_pro');
  expect(tokenProduct).toBe('anymd');
  expect(currencies[0]).toBe('USD');
  expect(Object.fromEntries(currencies.map((code) => [code, proAmount(code)]))).toEqual(expected);
  for (const invalid of [undefined, 'eur', ['USD', 'EUR'], 'ZZZ', 'constructor', '__proto__']) {
    expect(isCurrency(invalid)).toBe(false);
  }
});

test('prices have stable SSR labels and honor locale without changing the currency or amount', () => {
  expect(formatPrice(proAmount('EUR'), 'EUR')).toBe('€26.99');
  expect(formatPrice(proAmount('EUR'), 'EUR', ['fr-FR'])).toBe('26,99 €');
  expect(formatPrice(0, 'EUR', ['fr-FR'])).toBe('0 €');
  expect(formatPrice(proAmount('JPY'), 'JPY')).toBe('¥4,639');
  expect(formatPrice(proAmount('JPY'), 'JPY', 'ja-JP')).toBe('￥4,639');
  expect(formatPrice(proAmount('IDR'), 'IDR', 'id-ID')).toBe('Rp 529.000');
});

const portal = 'portal_sessions:userCreate';
const claim = 'checkout_sessions/cs_fixture:claim';
const noCustomer = { code: 'INVALID_STATE', details: [{ reason: 'no_customer' }] };

test('Money errors select purchase-specific copy only for the documented operation and problem', async () => {
  const money = {
    origin: 'https://money.example.invalid',
    parent: 'orgs/org_fixture/projects/prj_fixture/envs/env_fixture',
    pk: 'fixture',
  };
  const cases = [
    [portal, 412, noCustomer, 412, 'no_customer'],
    [
      portal,
      412,
      { code: 'INVALID_STATE', details: [{ reason: 'merchant_disconnected' }] },
      412,
      'billing_unavailable',
    ],
    [portal, 403, { ...noCustomer, code: 'PERMISSION_DENIED' }, 403, 'billing_refused'],
    [
      portal,
      403,
      { error: { code: 'PERMISSION_DENIED', details: [{ reason: 'no_customer' }] } },
      403,
      'billing_refused',
    ],
    [portal, 404, { code: 'RESOURCE_NOT_FOUND' }, 404, 'billing_unavailable'],
    [portal, 429, { code: 'RATE_LIMITED' }, 429, 'rate_limited'],
    [portal, 503, noCustomer, 502, 'billing_unavailable'],
    [claim, 412, noCustomer, 412, 'billing_unavailable'],
    [claim, 409, { code: 'RESOURCE_ALREADY_EXISTS' }, 409, 'already_claimed'],
    [claim, 404, { code: 'RESOURCE_NOT_FOUND' }, 404, 'not_found'],
    ['checkout_sessions', 409, { code: 'RESOURCE_ALREADY_EXISTS' }, 409, 'billing_unavailable'],
  ] as const;
  for (const [path, status, body, expectedStatus, code] of cases) {
    const upstream = spyOn(globalThis, 'fetch').mockImplementation(async () =>
      Response.json(
        { ...body, detail: 'private upstream diagnostic' },
        { status, headers: { 'Content-Type': 'application/problem+json' } }
      )
    );
    try {
      const response = await moneyCall(money, path, 'fixture', {}).catch(failure);
      expect(response.status).toBe(expectedStatus);
      expect(await response.json()).toEqual({ code });
    } finally {
      upstream.mockRestore();
    }
  }
});
