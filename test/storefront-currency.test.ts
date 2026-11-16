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
  amounts: Record<string, string> = { USD: '2900', HKD: '22900' },
  options: {
    search?: string;
    storedCurrency?: string;
    languages?: string[];
    storedAttribution?: Record<string, string>;
  } = {}
) {
  const calls: Request[] = [];
  const attributes = new Map([['data-keel-money-checkout', JSON.stringify(config)]]);
  const storage = new Map([['keel:money:attempt-nonce', 'same-offline-attempt']]);
  if (options.storedAttribution) {
    storage.set('anymd_pro_attribution', JSON.stringify(options.storedAttribution));
  }
  if (options.storedCurrency) storage.set('keel:money:currency', options.storedCurrency);
  const currencyListeners = new Map<string, () => void>();
  type CurrencyOption = { value: string; textContent: string; selected: boolean };
  const currencySelect = {
    value: '',
    options: [] as CurrencyOption[],
    replaceChildren: (...nodes: CurrencyOption[]) => {
      currencySelect.options = nodes;
    },
    addEventListener: (name: string, listener: () => void) => currencyListeners.set(name, listener),
  };
  const currencyWrap = { setAttribute: () => {}, removeAttribute: () => {} };
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
    __keelSelectCurrency: undefined as undefined | ((code: string) => boolean),
    dispatchEvent: () => true,
    hasAttribute: (name: string) => attributes.has(name),
    setAttribute: (name: string, value: string) => attributes.set(name, value),
    getAttribute: (name: string) => attributes.get(name) ?? null,
    querySelector: (selector: string) => {
      if (selector.includes('data-keel-money-field="price"')) return select;
      if (selector.includes('data-keel-money-field="currency"')) return currencySelect;
      if (selector === '[data-keel-money-currency-wrap]') return currencyWrap;
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
  class MockCustomEvent<T = unknown> {
    type: string;
    detail: T | undefined;
    bubbles: boolean;
    constructor(type: string, init?: { bubbles?: boolean; detail?: T }) {
      this.type = type;
      this.bubbles = Boolean(init?.bubbles);
      this.detail = init?.detail;
    }
  }
  const sandbox = {
    // Avoid host timezone influencing the catalogue-default fallback case.
    Intl: {
      NumberFormat: Intl.NumberFormat,
      Locale: Intl.Locale,
      DateTimeFormat: () => ({ resolvedOptions: () => ({ timeZone: 'UTC' }) }),
    },
    URL,
    URLSearchParams,
    TextEncoder,
    CustomEvent: MockCustomEvent,
    crypto: webcrypto,
    document: {
      querySelectorAll: () => [],
      querySelector: () => null,
      createElement: () => ({ value: '', textContent: '', selected: false }),
    },
    window: {
      navigator: { languages: options.languages ?? [] },
      localStorage: {
        getItem: (key: string) => storage.get(key) ?? null,
        setItem: (key: string, value: string) => storage.set(key, value),
      },
      location: {
        search: options.search ?? '',
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
  const exportExpr = '({ mountCheckout, extractAttributionMetadata, appendAttributionParams })';
  const exportsObj = runInNewContext(`${source.replace(/^export /gm, '')}\n${exportExpr}`, sandbox);
  await exportsObj.mountCheckout(root);
  const submit = async () => {
    const listener = listeners.get('submit');
    if (!listener) throw new Error('Checkout submit handler was not mounted');
    await listener({ preventDefault: () => {} });
  };
  return {
    calls,
    currencySelect,
    selectCurrency: (code: string) => root.__keelSelectCurrency?.(code),
    changeCurrency: (code: string) => {
      currencySelect.value = code;
      currencyListeners.get('change')?.();
    },
    attributes,
    display,
    button,
    submit,
    storage,
    extractAttributionMetadata: exportsObj.extractAttributionMetadata as (
      search?: string
    ) => Record<string, string>,
    appendAttributionParams: exportsObj.appendAttributionParams as (
      rawUrl: string,
      attribution: Record<string, string>
    ) => string,
  };
}

function post(checkout: Awaited<ReturnType<typeof mount>>) {
  const request = checkout.calls.find((call) => call.method === 'POST');
  if (!request?.body) throw new Error('Missing offline checkout request');
  expect(request.url).toBe(
    `${config.api_origin}/v1/money/${config.parent}/checkout_sessions:clientCreate`
  );
  return { body: JSON.parse(request.body), key: request.headers['Idempotency-Key'] };
}

const catalogue = JSON.parse(
  readFileSync(new URL('../config/money-catalogue.json', import.meta.url), 'utf8')
);
const catalogueAmounts: Record<string, string> = catalogue.spec.products
  .find((product: { key: string }) => product.key === config.product)
  .prices.find((price: { key: string }) => price.key === config.prices[0].key).unit_amounts;

describe('generated Keel storefront currency', () => {
  test('native selector offers all 16 catalogue currencies and submits each visible price', async () => {
    const checkout = await mount('USD', catalogueAmounts);
    expect(checkout.currencySelect.options).toHaveLength(16);
    expect(checkout.currencySelect.options.map((option) => option.value).sort()).toEqual(
      Object.keys(catalogueAmounts).sort()
    );
    const identities = new Set<string>();
    for (const [code, raw] of Object.entries(catalogueAmounts)) {
      checkout.changeCurrency(code);
      expect(checkout.currencySelect.value).toBe(code);
      expect(checkout.attributes.get('data-keel-money-currency')).toBe(code);
      const exponent = ['JPY', 'KRW', 'VND'].includes(code) ? 0 : 2;
      const formatted = new Intl.NumberFormat(undefined, {
        style: 'currency',
        currency: code,
        minimumFractionDigits: exponent,
        maximumFractionDigits: exponent,
      }).format(Number(raw) / (exponent === 0 ? 1 : 100));
      expect(checkout.display.textContent).toContain(formatted);
      expect(checkout.storage.get('keel:money:currency')).toBe(code);
      await checkout.submit();
      const last = checkout.calls.filter((call) => call.method === 'POST').at(-1);
      expect(JSON.parse(last?.body ?? '{}').currency_code).toBe(code);
      identities.add(last?.headers['Idempotency-Key'] ?? '');
    }
    expect(identities.size).toBe(16);
  });

  test('currency precedence is query, saved choice, locale, then catalogue default', async () => {
    const cases = [
      { search: '?currency=jpy', storedCurrency: 'HKD', languages: ['en-GB'], expected: 'JPY' },
      { search: '?currency=INVALID', storedCurrency: 'HKD', languages: ['en-GB'], expected: 'HKD' },
      { storedCurrency: 'INVALID', languages: ['en-GB'], expected: 'GBP' },
      { languages: [], expected: 'USD' },
    ];
    for (const { expected, ...options } of cases) {
      const checkout = await mount('USD', catalogueAmounts, options);
      expect(checkout.currencySelect.value).toBe(expected);
    }
  });

  test('invalid selection preserves visible price, request currency, and saved choice', async () => {
    const checkout = await mount('USD', catalogueAmounts);
    expect(checkout.selectCurrency('hkd')).toBe(true);
    const before = checkout.display.textContent;
    checkout.changeCurrency('INVALID');
    expect(checkout.selectCurrency('INVALID')).toBe(false);
    expect(checkout.currencySelect.value).toBe('HKD');
    expect(checkout.display.textContent).toBe(before);
    expect(checkout.storage.get('keel:money:currency')).toBe('HKD');
    await checkout.submit();
    expect(post(checkout).body.currency_code).toBe('HKD');
  });

  test('render hydrates only the catalogue and does not create an order', async () => {
    const checkout = await mount('HKD');
    expect(checkout.calls.map((call) => call.method)).toEqual(['GET']);
    expect(checkout.attributes.get('data-keel-money-catalog-ready')).toBe('1');
    expect(checkout.button.disabled).toBe(false);
    expect(checkout.display.textContent).toContain('229.00');
  });

  test('request currency follows visible catalogue default, not the first amount', async () => {
    const checkout = await mount('HKD');
    await checkout.submit();
    const request = post(checkout);
    expect(request.body.currency_code).toBe('HKD');
    expect(checkout.display.textContent).toContain('229.00');
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
    expect(hkd.display.textContent).toContain('229.00');
  });

  test('unavailable catalogue currency fails closed without a checkout write', async () => {
    const checkout = await mount('EUR');
    expect(checkout.button.disabled).toBe(true);
    expect(checkout.attributes.get('data-keel-money-catalog-ready')).toBe('0');
    await checkout.submit();
    expect(checkout.calls.map((call) => call.method)).toEqual(['GET']);
  });
});

describe('generated Keel storefront SEO, layout, and attribution', () => {
  test('pricing.html fixes title, og:image, paid JSON-LD, and above-the-fold checkout', () => {
    const expectedTitle = 'anymd Pro: video moments you can cite, quotes you can check';
    const title = html.match(/<title>([^<]+)<\/title>/)?.[1] ?? '';
    expect(title).toBe(expectedTitle);
    expect(title).not.toContain('· anymd Pro');
    expect(html).toContain(`<meta property="og:title" content="${expectedTitle}">`);
    expect(html).toContain(
      '<meta property="og:image" content="https://sylphxai.github.io/anymd/og-pro.png">'
    );
    expect(html).toContain(
      '<meta name="twitter:image" content="https://sylphxai.github.io/anymd/og-pro.png">'
    );
    expect(html).toContain('<meta property="og:type" content="website">');
    expect(html).toContain('type="application/ld+json"');
    expect(html).toContain('"isAccessibleForFree":false');
    expect(html).not.toContain('"price":"0"');
    expect(html).not.toContain('id="docs-nav"');
    expect(html).not.toContain('id="docs-toc"');
    const checkoutIdx = html.indexOf('id="money-checkout-pricing"');
    const detailsIdx = html.indexOf('id="why-it-matters"');
    expect(checkoutIdx).toBeGreaterThan(-1);
    expect(detailsIdx).toBeGreaterThan(checkoutIdx);
    expect(html).not.toContain('Money');
    expect(html).toContain('Buy a 100-year Pro licence with one payment.');
    const recoverHtml = readFileSync(new URL('recover.html', prebuilt), 'utf8');
    expect(html).toContain('Apple Pay · Google Pay · Link');
    expect(html).toContain('anymd-wallet-trust');
    expect(html).toContain('data-anymd-ga4="1"');
    expect(html).toContain('G-R4TD2Z8RR7');
    expect(html).not.toContain('src="https://www.googletagmanager.com');
    expect(recoverHtml).not.toContain('Money');
    expect(recoverHtml).toContain('data-anymd-ga4="1"');
    expect(recoverHtml).toContain('G-R4TD2Z8RR7');
    expect(recoverHtml).not.toContain('src="https://www.googletagmanager.com');
  });

  test('money runtime forwards click IDs and UTMs into metadata and return URLs', async () => {
    const search = [
      '?gclid=g_123',
      'gbraid=gb_456',
      'wbraid=wb_789',
      'fbclid=fb_012',
      'utm_source=google',
      'utm_medium=cpc',
      'utm_campaign=pro',
      'entry_surface=seo_verifiable-video-and-cite-check-ai-agents',
    ].join('&');
    const checkout = await mount('USD', { USD: '2900' }, { search });
    expect(checkout.extractAttributionMetadata(search)).toEqual({
      utm_source: 'google',
      utm_medium: 'cpc',
      utm_campaign: 'pro',
      gclid: 'g_123',
      gbraid: 'gb_456',
      wbraid: 'wb_789',
      fbclid: 'fb_012',
      entry_surface: 'seo_verifiable-video-and-cite-check-ai-agents',
    });

    await checkout.submit();
    const request = post(checkout);
    expect(request.body.metadata).toEqual({
      utm_source: 'google',
      utm_medium: 'cpc',
      utm_campaign: 'pro',
      gclid: 'g_123',
      gbraid: 'gb_456',
      wbraid: 'wb_789',
      fbclid: 'fb_012',
      entry_surface: 'seo_verifiable-video-and-cite-check-ai-agents',
    });
    expect(request.body.success_url).toContain(
      'entry_surface=seo_verifiable-video-and-cite-check-ai-agents'
    );
    expect(request.body.success_url).toContain('gclid=g_123');
    expect(request.body.success_url).toContain('gbraid=gb_456');
    expect(request.body.success_url).toContain('wbraid=wb_789');
    expect(request.body.success_url).toContain('fbclid=fb_012');
    expect(request.body.success_url).toContain('utm_source=google');
    expect(request.body.success_url).toContain('{CHECKOUT_SESSION_ID}');
    expect(request.body.cancel_url).toContain('gclid=g_123');
    expect(request.body.cancel_url).toContain('fbclid=fb_012');
  });

  test('money runtime restores sessionStorage attribution when query is empty', async () => {
    const checkout = await mount(
      'USD',
      { USD: '2900' },
      {
        search: '',
        storedAttribution: {
          gclid: 'from_pro_page',
          fbclid: 'fb_from_pro',
          utm_source: 'pro_landing',
        },
      }
    );
    await checkout.submit();
    const request = post(checkout);
    expect(request.body.metadata).toEqual({
      utm_source: 'pro_landing',
      gclid: 'from_pro_page',
      fbclid: 'fb_from_pro',
    });
    expect(request.body.success_url).toContain('gclid=from_pro_page');
    expect(request.body.success_url).toContain('fbclid=fb_from_pro');
  });

  test('storefront GA4 script stays fail-closed pre-consent and tracks page_view, begin_checkout, and deduped purchase post-consent', () => {
    const scriptMatch = html.match(/<script data-anymd-ga4="1">([\s\S]*?)<\/script>/);
    if (!scriptMatch) throw new Error('Missing data-anymd-ga4 script in pricing.html');
    const scriptBody = scriptMatch[1];

    const localMap = new Map<string, string>();
    const sessionMap = new Map<string, string>([
      ['anymd_pro_attribution', JSON.stringify({ entry_surface: 'seo_video-evidence' })],
    ]);
    const appendedSrcs: string[] = [];
    const docListeners = new Map<string, Array<(ev: unknown) => void>>();
    const consentBtnAttrs = new Map<string, Map<string, string>>([
      [
        'all',
        new Map([
          ['data-anymd-consent', 'all'],
          ['aria-pressed', 'false'],
        ]),
      ],
      [
        'analytics',
        new Map([
          ['data-anymd-consent', 'analytics'],
          ['aria-pressed', 'false'],
        ]),
      ],
      [
        'none',
        new Map([
          ['data-anymd-consent', 'none'],
          ['aria-pressed', 'false'],
        ]),
      ],
    ]);
    const consentBtnHandlers = new Map<string, () => void>();
    const buttons = Array.from(consentBtnAttrs.entries()).map(([choice, attrs]) => ({
      getAttribute: (k: string) => attrs.get(k) ?? null,
      setAttribute: (k: string, v: string) => attrs.set(k, v),
      addEventListener: (_type: string, fn: () => void) => consentBtnHandlers.set(choice, fn),
    }));
    const banner = {
      hidden: true,
      querySelectorAll: () => buttons,
    };
    const dataLayer: unknown[] = [];
    const win: Record<string, unknown> = { dataLayer };
    const doc = {
      getElementById: (id: string) => (id === 'anymd-sf-consent' ? banner : null),
      querySelector: (sel: string) =>
        sel === '[data-keel-money-field="price"]' ? { value: 'pro_single' } : null,
      createElement: () => ({ async: false, src: '' }),
      head: { appendChild: (node: { src: string }) => appendedSrcs.push(node.src) },
      addEventListener: (type: string, fn: (ev: unknown) => void) => {
        const list = docListeners.get(type) ?? [];
        list.push(fn);
        docListeners.set(type, list);
      },
    };
    class MockEl {
      getAttribute(k: string) {
        return k === 'data-keel-money-form' ? 'checkout' : null;
      }
      closest() {
        return null;
      }
    }
    const checkoutForm = new MockEl();

    runInNewContext(scriptBody, {
      URLSearchParams,
      Element: MockEl,
      window: win,
      document: doc,
      location: {
        origin: 'https://sylphxai.github.io',
        pathname: '/anymd/pricing',
        search: '',
        href: 'https://sylphxai.github.io/anymd/pricing',
      },
      localStorage: {
        getItem: (k: string) => localMap.get(k) ?? null,
        setItem: (k: string, v: string) => localMap.set(k, v),
      },
      sessionStorage: {
        getItem: (k: string) => sessionMap.get(k) ?? null,
        setItem: (k: string, v: string) => sessionMap.set(k, v),
      },
    });

    // Pre-consent: zero gtag script, zero events even if checkout submits or payment succeeds
    expect(banner.hidden).toBe(false);
    expect(appendedSrcs).toHaveLength(0);
    expect(dataLayer).toHaveLength(0);
    for (const fn of docListeners.get('submit') ?? []) fn({ target: checkoutForm });
    expect(dataLayer).toHaveLength(0);

    // Grant analytics consent
    consentBtnHandlers.get('analytics')?.();
    expect(banner.hidden).toBe(true);
    expect(localMap.get('anymd_pro_consent')).toBe('analytics');
    expect(appendedSrcs).toEqual(['https://www.googletagmanager.com/gtag/js?id=G-R4TD2Z8RR7']);

    for (const fn of docListeners.get('submit') ?? []) fn({ target: checkoutForm });
    const paidEvent = {
      detail: { status: 'succeeded', receipt: { Store: { store: 'web', data: 'cs_test_sf_999' } } },
    };
    for (const fn of docListeners.get('keel:money:purchase') ?? []) fn(paidEvent);
    for (const fn of docListeners.get('keel:money:purchase') ?? []) fn(paidEvent);

    const calls = dataLayer.map((args) => Array.from(args as ArrayLike<unknown>));
    const pageViews = calls.filter((c) => c[0] === 'event' && c[1] === 'page_view');
    const beginCheckouts = calls.filter((c) => c[0] === 'event' && c[1] === 'begin_checkout');
    const purchases = calls.filter((c) => c[0] === 'event' && c[1] === 'purchase');

    expect(pageViews).toHaveLength(1);
    expect(pageViews[0]?.[2]).toMatchObject({
      send_to: 'G-R4TD2Z8RR7',
      entry_surface: 'seo_video-evidence',
      plan: 'pro',
      plan_sku: 'anymd_pro',
      billing_interval: 'one_time',
      option: 'pro_single',
      checkout_trigger: 'page_view',
    });
    expect(beginCheckouts).toHaveLength(1);
    expect(beginCheckouts[0]?.[2]).toMatchObject({
      entry_surface: 'seo_video-evidence',
      plan: 'pro',
      plan_sku: 'anymd_pro',
      billing_interval: 'one_time',
      option: 'pro_single',
      checkout_trigger: 'pricing_checkout_form',
    });
    expect(purchases).toHaveLength(1);
    expect(purchases[0]?.[2]).toMatchObject({
      transaction_id: 'cs_test_sf_999',
      entry_surface: 'seo_video-evidence',
      plan: 'pro',
      plan_sku: 'anymd_pro',
      billing_interval: 'one_time',
      option: 'pro_single',
      checkout_trigger: 'stripe_redirect',
    });
    expect(sessionMap.get('anymd_pro_purchase_cs_test_sf_999')).toBe('1');
  });

  test('storefront script purges _ga* cookies on Reject and on initial none consent across pricing and recover', () => {
    const recoverHtml = readFileSync(new URL('recover.html', prebuilt), 'utf8');
    for (const [pageName, pageHtml, pathname] of [
      ['pricing', html, '/anymd/pricing'],
      ['recover', recoverHtml, '/anymd/recover'],
    ] as const) {
      const scriptMatch = pageHtml.match(/<script data-anymd-ga4="1">([\s\S]*?)<\/script>/);
      if (!scriptMatch) throw new Error(`Missing data-anymd-ga4 script in ${pageName}.html`);
      const scriptBody = scriptMatch[1];

      type CookieEntry = { name: string; value: string; domain: string; path: string };
      const entries = new Map<string, CookieEntry>();
      const seed = (name: string, value: string, domain = '', path = '/') => {
        entries.set(`${name}\0${domain}\0${path}`, { name, value, domain, path });
      };
      const writeCookie = (raw: string) => {
        const parts = raw.split(';').map((p) => p.trim());
        const [nv, ...attrs] = parts;
        const eq = nv.indexOf('=');
        if (eq === -1) return;
        const name = nv.slice(0, eq).trim();
        const value = nv.slice(eq + 1).trim();
        let domain = '';
        let path = '/';
        let expired = false;
        for (const attr of attrs) {
          const [kRaw, vRaw = ''] = attr.split('=');
          const k = kRaw.trim().toLowerCase();
          const v = vRaw.trim();
          if (k === 'domain') domain = v.toLowerCase();
          if (k === 'path') path = v;
          if (k === 'max-age' && Number(v) <= 0) expired = true;
          if (k === 'expires' && Date.parse(v) <= 0) expired = true;
        }
        const key = `${name}\0${domain}\0${path}`;
        if (expired) {
          entries.delete(key);
        } else {
          entries.set(key, { name, value, domain, path });
        }
      };
      const readCookie = () =>
        Array.from(entries.values())
          .map((c) => `${c.name}=${c.value}`)
          .join('; ');

      const mountPage = (initialConsent?: string) => {
        const localMap = new Map<string, string>();
        if (initialConsent) localMap.set('anymd_pro_consent', initialConsent);
        const sessionMap = new Map<string, string>();
        const consentBtnAttrs = new Map<string, Map<string, string>>([
          [
            'all',
            new Map([
              ['data-anymd-consent', 'all'],
              ['aria-pressed', 'false'],
            ]),
          ],
          [
            'analytics',
            new Map([
              ['data-anymd-consent', 'analytics'],
              ['aria-pressed', 'false'],
            ]),
          ],
          [
            'none',
            new Map([
              ['data-anymd-consent', 'none'],
              ['aria-pressed', 'false'],
            ]),
          ],
        ]);
        const consentBtnHandlers = new Map<string, () => void>();
        const buttons = Array.from(consentBtnAttrs.entries()).map(([choice, attrs]) => ({
          getAttribute: (k: string) => attrs.get(k) ?? null,
          setAttribute: (k: string, v: string) => attrs.set(k, v),
          addEventListener: (_type: string, fn: () => void) => consentBtnHandlers.set(choice, fn),
        }));
        const banner = { hidden: true, querySelectorAll: () => buttons };
        const dataLayer: unknown[] = [];
        const win: Record<string, unknown> = { dataLayer };
        const doc = {
          get cookie() {
            return readCookie();
          },
          set cookie(v: string) {
            writeCookie(v);
          },
          getElementById: (id: string) => (id === 'anymd-sf-consent' ? banner : null),
          querySelector: () => null,
          createElement: () => ({ async: false, src: '' }),
          head: { appendChild: () => {} },
          addEventListener: () => {},
        };
        class MockEl {}
        runInNewContext(scriptBody, {
          URL,
          URLSearchParams,
          Element: MockEl,
          window: win,
          document: doc,
          location: {
            origin: 'https://sylphxai.github.io',
            hostname: 'sylphxai.github.io',
            pathname,
            search: '',
            href: `https://sylphxai.github.io${pathname}`,
          },
          localStorage: {
            getItem: (k: string) => localMap.get(k) ?? null,
            setItem: (k: string, v: string) => localMap.set(k, v),
          },
          sessionStorage: {
            getItem: (k: string) => sessionMap.get(k) ?? null,
            setItem: (k: string, v: string) => sessionMap.set(k, v),
          },
        });
        return { localMap, consentBtnHandlers, dataLayer, win, doc };
      };

      // 1. Accept -> simulated GA cookies -> Reject clears all GA cookies immediately.
      entries.clear();
      const session = mountPage();
      session.consentBtnHandlers.get('all')?.();
      expect(session.localMap.get('anymd_pro_consent')).toBe('all');
      seed('_ga', 'GA1.1.111.222', '.sylphxai.github.io', '/');
      seed('_ga_R4TD2Z8RR7', 'GS2.1.s111', '.sylphxai.github.io', '/');
      seed('_gid', 'GA1.2.333', '', '/anymd');
      seed('_gat_gtag_G_R4TD2Z8RR7', '1', 'sylphxai.github.io', '/anymd/');
      seed('session_pref', 'keep', '', '/');

      session.consentBtnHandlers.get('none')?.();
      expect(session.localMap.get('anymd_pro_consent')).toBe('none');
      expect(session.win['ga-disable-G-R4TD2Z8RR7']).toBe(true);
      const calls = session.dataLayer.map((args) => Array.from(args as ArrayLike<unknown>));
      expect(calls[calls.length - 1]).toEqual([
        'consent',
        'update',
        {
          ad_storage: 'denied',
          ad_user_data: 'denied',
          ad_personalization: 'denied',
          analytics_storage: 'denied',
        },
      ]);
      expect(session.doc.cookie).toBe('session_pref=keep');

      // 2. Initial load with stored consent 'none' purges pre-existing GA cookies.
      seed('_ga', 'GA1.1.stale', '.sylphxai.github.io', '/');
      seed('_ga_R4TD2Z8RR7', 'GS2.1.stale', '.sylphxai.github.io', '/');
      const noneSession = mountPage('none');
      expect(noneSession.win['ga-disable-G-R4TD2Z8RR7']).toBe(true);
      expect(noneSession.doc.cookie).toBe('session_pref=keep');
    }
  });

  test('storefront CSS overrides Keel font variables so __keel_absent_alias_ui never resolves on body', () => {
    const recoverHtml = readFileSync(new URL('recover.html', prebuilt), 'utf8');
    for (const pageHtml of [html, recoverHtml]) {
      const styleMatch = pageHtml.match(/<style data-anymd-storefront>([\s\S]*?)<\/style>/);
      if (!styleMatch) throw new Error('Missing <style data-anymd-storefront>');
      const css = styleMatch[1];
      expect(css).toContain('--keel-font-ui: var(--sf-sans);');
      expect(css).toContain('--keel-asset-font-ui: var(--sf-sans);');
      expect(css).toContain('font-family: var(--sf-sans);');
      expect(css).not.toContain('__keel_absent_alias_ui');
    }
  });
});
