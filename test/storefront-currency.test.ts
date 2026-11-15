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
  amounts: Record<string, string> = { USD: '2900', HKD: '22600' },
  options: { search?: string; storedAttribution?: Record<string, string> } = {}
) {
  const calls: Request[] = [];
  const attributes = new Map([['data-keel-money-checkout', JSON.stringify(config)]]);
  const storage = new Map([['keel:money:attempt-nonce', 'same-offline-attempt']]);
  if (options.storedAttribution) {
    storage.set('anymd_pro_attribution', JSON.stringify(options.storedAttribution));
  }
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
    dispatchEvent: () => true,
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
    URL,
    URLSearchParams,
    TextEncoder,
    CustomEvent: MockCustomEvent,
    crypto: webcrypto,
    window: {
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

describe('generated Keel storefront SEO, layout, and attribution', () => {
  test('pricing.html fixes title, og:image, paid JSON-LD, and above-the-fold checkout', () => {
    const expectedTitle = 'anymd Pro: verifiable video evidence and cite-check for AI agents';
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
    expect(html).toContain('One-time 100-year offline licence');
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
});
