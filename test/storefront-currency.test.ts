import { describe, expect, test } from 'bun:test';
import { webcrypto } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const prebuilt = new URL('../docs/storefront/prebuilt/', import.meta.url);
const html = readFileSync(new URL('pricing.html', prebuilt), 'utf8');
const runtimePath = html.match(/src="\/anymd\/(_keel\/money\.[^"]+\.js)"/)?.[1];
const rawConfig = html.match(/data-keel-money-checkout="([^"]+)"/)?.[1];
if (!runtimePath || !rawConfig) throw new Error('Missing generated Money checkout');
const source = readFileSync(new URL(runtimePath, prebuilt), 'utf8');
const config = JSON.parse(
  rawConfig
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&')
);

type Request = { url: string; method: string; headers: Record<string, string>; body?: string };
type Submit = (event: { preventDefault(): void }) => Promise<void>;

// Execute the referenced, committed engine bytes in an isolated browser-shaped
// context. No request can reach the network: even submit is a refusing fixture.
async function mount(
  currency: string,
  amounts: Record<string, string> = { USD: '2900', HKD: '22600' }
) {
  const calls: Request[] = [];
  const attributes = new Map([['data-keel-money-checkout', JSON.stringify(config)]]);
  const storage = new Map([['keel:money:attempt-nonce', 'same-offline-attempt']]);
  const listeners = new Map<string, Submit>();
  const option = {
    value: config.prices[0].id || `${config.prices[0].key}:${config.prices[0].quantity}`,
    textContent: config.prices[0].label,
    getAttribute: () => config.prices[0].label,
  };
  const select = {
    value: option.value,
    selectedOptions: [option],
    querySelectorAll: () => [option],
    addEventListener: () => {},
  };
  const display = { textContent: '' };
  const status = { textContent: '' };
  const button = {
    disabled: true,
    setAttribute: () => {},
    removeAttribute: () => {},
    getAttribute: () => null,
  };
  const root = {
    hasAttribute: (name: string) => attributes.has(name),
    setAttribute: (name: string, value: string) => attributes.set(name, value),
    getAttribute: (name: string) => attributes.get(name) ?? null,
    querySelector: (selector: string) => {
      if (selector.includes('data-keel-money-field="price"')) return select;
      if (selector.includes('data-keel-money-field="email"'))
        return { value: 'buyer@example.invalid' };
      if (selector.includes('data-keel-money-field="consent"')) return { checked: true };
      if (selector === '[data-keel-money-price-display]') return display;
      if (selector === '[data-keel-money-status]') return status;
      if (selector === '[data-keel-money-submit="checkout"]') return button;
      if (selector === '[data-keel-money-form="checkout"]') {
        return {
          addEventListener: (name: string, listener: Submit) => listeners.set(name, listener),
        };
      }
      return null;
    },
  };
  const sandbox = {
    URL,
    URLSearchParams,
    TextEncoder,
    crypto: webcrypto,
    window: {
      location: {
        search: '',
        assign: () => {
          throw new Error('Unexpected checkout redirect');
        },
      },
      sessionStorage: {
        getItem: (key: string) => storage.get(key) ?? null,
        setItem: (key: string, value: string) => storage.set(key, value),
      },
    },
    fetch: async (url: string, init: Omit<Request, 'url'>) => {
      calls.push({ url, ...init });
      if (init.method !== 'GET') return { ok: false, status: 503, json: async () => ({}) };
      if (url !== `${config.api_origin}/v1/money/${config.parent}/catalog`) {
        throw new Error(`Unexpected fixture GET: ${url}`);
      }
      return {
        ok: true,
        json: async () => ({
          spec: {
            products: [
              {
                key: config.product,
                prices: [
                  {
                    key: config.prices[0].key,
                    default_currency: currency,
                    unit_amounts: amounts,
                  },
                ],
              },
            ],
          },
        }),
      };
    },
  };
  const mountCheckout = runInNewContext(
    `${source.replace(/^export /gm, '')}\nmountCheckout`,
    sandbox
  );
  await mountCheckout(root);
  const submit = async () => {
    const listener = listeners.get('submit');
    if (!listener) throw new Error('Checkout submit handler was not mounted');
    await listener({ preventDefault: () => {} });
  };
  return { calls, attributes, display, button, submit };
}

function post(checkout: Awaited<ReturnType<typeof mount>>) {
  const request = checkout.calls.find((call) => call.method === 'POST');
  if (!request?.body) throw new Error('Missing offline checkout request');
  expect(request.url).toBe(
    `${config.api_origin}/v1/money/${config.parent}/checkout_sessions:clientCreate`
  );
  return { body: JSON.parse(request.body), key: request.headers['Idempotency-Key'] };
}

describe('generated Keel storefront currency', () => {
  test('render hydrates only the catalogue and does not create an order', async () => {
    const checkout = await mount('HKD');
    expect(checkout.calls.map((call) => call.method)).toEqual(['GET']);
    expect(checkout.attributes.get('data-keel-money-catalog-ready')).toBe('1');
    expect(checkout.button.disabled).toBe(false);
    expect(checkout.display.textContent).toContain('226.00');
  });

  test('request currency follows visible catalogue default, not the first amount', async () => {
    const checkout = await mount('HKD');
    await checkout.submit();
    const request = post(checkout);
    expect(request.body.currency_code).toBe('HKD');
    expect(checkout.display.textContent).toContain('226.00');
    await checkout.submit();
    const attempts = checkout.calls.filter((call) => call.method === 'POST');
    expect(attempts).toHaveLength(2);
    expect(attempts[1]?.headers['Idempotency-Key']).toBe(request.key);
  });

  test('changing only the catalogue currency changes the idempotency identity', async () => {
    const usd = await mount('USD');
    const hkd = await mount('HKD');
    await usd.submit();
    await hkd.submit();
    expect(post(usd).body.currency_code).toBe('USD');
    expect(post(hkd).body.currency_code).toBe('HKD');
    expect(post(usd).key).not.toBe(post(hkd).key);
    expect(usd.display.textContent).toContain('29.00');
    expect(hkd.display.textContent).toContain('226.00');
  });

  test('unavailable catalogue currency fails closed without a checkout write', async () => {
    const checkout = await mount('EUR');
    expect(checkout.button.disabled).toBe(true);
    expect(checkout.attributes.get('data-keel-money-catalog-ready')).toBe('0');
    await checkout.submit();
    expect(checkout.calls.map((call) => call.method)).toEqual(['GET']);
  });
});
