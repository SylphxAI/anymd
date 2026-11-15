import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import config from '../docs/.vitepress/pro/config.json';
import {
  _resetTrackingForTests,
  ATTRIBUTION_STORAGE_KEY,
  appendAttributionToUrl,
  buyHref,
  clickId,
  extractAttribution,
  isPlaceholder,
  isPricingHref,
  loadTags,
  mergedAttribution,
  PURCHASE_STORAGE_PREFIX,
  persistAttribution,
  STORE_KEY,
  trackBeginCheckout,
  trackingEnabled,
  trackPurchase,
  transactionId,
  updateConsent,
} from '../docs/.vitepress/pro/tracking';

describe('pro config', () => {
  test('placeholders are recognised, including the payment link placeholder', () => {
    expect(isPlaceholder('https://PRO_PAYMENT_LINK')).toBe(true);
    expect(isPlaceholder('G-XXXX')).toBe(true);
    expect(isPlaceholder('AW-XXXX')).toBe(true);
    expect(isPlaceholder('')).toBe(true);
    expect(isPlaceholder('https://buy.stripe.com/abc')).toBe(false);
  });

  test('GA4 measurement ID G-R4TD2Z8RR7 is wired and placeholder IDs are absent', () => {
    const raw = readFileSync(
      new URL('../docs/.vitepress/pro/config.json', import.meta.url),
      'utf8'
    );
    expect(raw).not.toContain('G-XXXX');
    expect(raw).not.toContain('AW-XXXX');
    expect(config.ga4MeasurementId).toBe('G-R4TD2Z8RR7');
    expect(trackingEnabled()).toBe(true);
  });

  test('the Buy button and site CTAs link straight to /pricing', () => {
    expect(config.buyUrl).toBe('https://sylphxai.github.io/anymd/pricing');
    expect(isPlaceholder(config.buyUrl)).toBe(false);
    const button = readFileSync(
      new URL('../docs/.vitepress/theme/components/ProBuy.vue', import.meta.url),
      'utf8'
    );
    expect(button).toContain('const href = ref(buyUrl)');
    expect(button).toContain('target="_self"');
    expect(button).not.toMatch(/buyMailto|buyReady|mailto:/);
    expect(button).toContain("label ?? 'Buy anymd Pro'");
    expect(button).not.toContain('US$29');

    const indexMd = readFileSync(new URL('../docs/index.md', import.meta.url), 'utf8');
    expect(indexMd).toContain('[anymd Pro](/pricing)');
    expect(indexMd).not.toContain('(/pro)');

    const footer = readFileSync(
      new URL('../docs/.vitepress/theme/components/SiteFooter.vue', import.meta.url),
      'utf8'
    );
    expect(footer).toContain("withBase('/pricing')");
    expect(footer).not.toContain("withBase('/pro')");

    const vpConfig = readFileSync(new URL('../docs/.vitepress/config.ts', import.meta.url), 'utf8');
    expect(vpConfig).toContain("link: '/pricing'");
    expect(vpConfig).not.toContain("link: '/pro'");
    expect(vpConfig).toContain("url: 'pricing'");
    expect(vpConfig).toContain('isAccessibleForFree: false');
    expect(vpConfig).toContain('/use-cases/pdf-and-office-to-markdown-mcp');
    expect(vpConfig).toContain('/use-cases/verifiable-video-and-cite-check-ai-agents');

    const proMd = readFileSync(new URL('../docs/pro.md', import.meta.url), 'utf8');
    expect(proMd).not.toContain('Money');
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

  test('GA4 is fail-closed before consent and emits page_view, begin_checkout, and deduped purchase after consent', () => {
    const localMap = new Map<string, string>();
    const sessionMap = new Map<string, string>();
    const appendedSrcs: string[] = [];
    const dataLayer: unknown[] = [];

    const prevWindow = (globalThis as Record<string, unknown>).window;
    const prevDocument = (globalThis as Record<string, unknown>).document;
    const prevLocation = (globalThis as Record<string, unknown>).location;
    const prevLocal = globalThis.localStorage;
    const prevSession = globalThis.sessionStorage;

    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      value: {
        getItem: (k: string) => localMap.get(k) ?? null,
        setItem: (k: string, v: string) => localMap.set(k, v),
      },
    });
    Object.defineProperty(globalThis, 'sessionStorage', {
      configurable: true,
      value: {
        getItem: (k: string) => sessionMap.get(k) ?? null,
        setItem: (k: string, v: string) => sessionMap.set(k, v),
      },
    });
    Object.defineProperty(globalThis, 'location', {
      configurable: true,
      value: {
        origin: 'https://sylphxai.github.io',
        pathname: '/anymd/pricing',
        search: '?entry_surface=seo_pdf-and-office',
        href: 'https://sylphxai.github.io/anymd/pricing?entry_surface=seo_pdf-and-office',
      },
    });
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: { dataLayer },
    });
    Object.defineProperty(globalThis, 'document', {
      configurable: true,
      value: {
        querySelector: () => null,
        createElement: () => ({ async: false, src: '' }),
        head: {
          appendChild: (node: { src: string }) => appendedSrcs.push(node.src),
        },
      },
    });

    try {
      _resetTrackingForTests();
      expect(loadTags('pro')).toBe(false);
      expect(appendedSrcs).toHaveLength(0);
      expect(dataLayer).toHaveLength(0);

      trackBeginCheckout('pro_buy_cta');
      trackPurchase('?session_id=cs_test_preconsent');
      expect(dataLayer).toHaveLength(0);
      expect(sessionMap.has(`${PURCHASE_STORAGE_PREFIX}cs_test_preconsent`)).toBe(false);

      updateConsent('analytics', 'pro');
      expect(localMap.get(STORE_KEY)).toBe('analytics');
      expect(appendedSrcs).toEqual(['https://www.googletagmanager.com/gtag/js?id=G-R4TD2Z8RR7']);

      const calls = dataLayer.map((args) => Array.from(args as ArrayLike<unknown>));
      const pageView = calls.find((c) => c[0] === 'event' && c[1] === 'page_view');
      expect(pageView?.[2]).toMatchObject({
        send_to: 'G-R4TD2Z8RR7',
        entry_surface: 'seo_pdf-and-office',
        plan: 'pro',
        plan_sku: 'anymd_pro',
        billing_interval: 'one_time',
        option: 'anymd_pro',
        checkout_trigger: 'page_view',
      });

      trackBeginCheckout('pro_buy_cta');
      trackPurchase('?session_id=cs_test_live123&entry_surface=seo_pdf-and-office');
      trackPurchase('?session_id=cs_test_live123&entry_surface=seo_pdf-and-office');

      const allCalls = dataLayer.map((args) => Array.from(args as ArrayLike<unknown>));
      const beginCalls = allCalls.filter((c) => c[0] === 'event' && c[1] === 'begin_checkout');
      const purchaseCalls = allCalls.filter((c) => c[0] === 'event' && c[1] === 'purchase');
      expect(beginCalls).toHaveLength(1);
      expect(beginCalls[0]?.[2]).toMatchObject({
        entry_surface: 'seo_pdf-and-office',
        plan: 'pro',
        plan_sku: 'anymd_pro',
        billing_interval: 'one_time',
        option: 'anymd_pro',
        checkout_trigger: 'pro_buy_cta',
      });
      expect(purchaseCalls).toHaveLength(1);
      expect(purchaseCalls[0]?.[2]).toMatchObject({
        transaction_id: 'cs_test_live123',
        entry_surface: 'seo_pdf-and-office',
        plan: 'pro',
        plan_sku: 'anymd_pro',
        billing_interval: 'one_time',
        option: 'anymd_pro',
        checkout_trigger: 'stripe_redirect',
      });
      expect(sessionMap.get(`${PURCHASE_STORAGE_PREFIX}cs_test_live123`)).toBe('1');
    } finally {
      _resetTrackingForTests();
      Object.defineProperty(globalThis, 'window', { configurable: true, value: prevWindow });
      Object.defineProperty(globalThis, 'document', { configurable: true, value: prevDocument });
      Object.defineProperty(globalThis, 'location', { configurable: true, value: prevLocation });
      Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: prevLocal });
      Object.defineProperty(globalThis, 'sessionStorage', {
        configurable: true,
        value: prevSession,
      });
    }
  });

  test('extractAttribution captures click IDs and UTM params safely', () => {
    const attr = extractAttribution(
      [
        '?gclid=g_123',
        'gbraid=gb_456',
        'wbraid=wb_789',
        'fbclid=fb.1.2_3',
        'utm_source=google',
        'utm_medium=cpc',
        'utm_campaign=launch',
        'utm_content=hero',
        'utm_term=pdf',
        'ignored=1',
      ].join('&')
    );
    expect(attr).toEqual({
      gclid: 'g_123',
      gbraid: 'gb_456',
      wbraid: 'wb_789',
      fbclid: 'fb.1.2_3',
      utm_source: 'google',
      utm_medium: 'cpc',
      utm_campaign: 'launch',
      utm_content: 'hero',
      utm_term: 'pdf',
    });
    expect(clickId('?gclid=g_123')).toBe('g_123');
    expect(clickId('?gclid=bad%20id')).toBe('');
  });

  test('buyHref and appendAttributionToUrl forward click IDs and UTMs to /pricing', () => {
    expect(buyHref('')).toBe(config.buyUrl);
    expect(buyHref('abc123')).toBe(`${config.buyUrl}?gclid=abc123`);
    expect(
      buyHref('?gclid=g1&gbraid=gb1&wbraid=wb1&fbclid=fb1&utm_source=ads&utm_campaign=pro')
    ).toBe(
      `${config.buyUrl}?gclid=g1&gbraid=gb1&wbraid=wb1&fbclid=fb1&utm_source=ads&utm_campaign=pro`
    );
    expect(
      appendAttributionToUrl('/anymd/pricing?utm_source=docs', {
        gclid: 'g1',
        fbclid: 'fb1',
        utm_source: 'override_ignored',
        utm_medium: 'cpc',
      })
    ).toBe('/anymd/pricing?utm_source=docs&gclid=g1&fbclid=fb1&utm_medium=cpc');
    expect(
      appendAttributionToUrl('https://buy.stripe.com/test123', {
        gclid: 'g1',
        utm_source: 'google',
      })
    ).toBe('https://buy.stripe.com/test123?client_reference_id=g1&utm_source=google');
    expect(isPricingHref('/anymd/pricing')).toBe(true);
    expect(isPricingHref('https://sylphxai.github.io/anymd/pricing?utm_source=docs')).toBe(true);
    expect(isPricingHref('/anymd/guide/getting-started')).toBe(false);
  });

  test('persistAttribution and mergedAttribution preserve click IDs across navigation', () => {
    const store = new Map<string, string>();
    const prevStorage = globalThis.sessionStorage;
    Object.defineProperty(globalThis, 'sessionStorage', {
      configurable: true,
      value: {
        getItem: (k: string) => store.get(k) ?? null,
        setItem: (k: string, v: string) => {
          store.set(k, v);
        },
      },
    });
    try {
      const saved = persistAttribution('?gclid=g_persist&fbclid=fb_persist&utm_source=meta');
      expect(saved).toEqual({
        gclid: 'g_persist',
        fbclid: 'fb_persist',
        utm_source: 'meta',
      });
      expect(store.has(ATTRIBUTION_STORAGE_KEY)).toBe(true);
      expect(mergedAttribution('')).toEqual({
        gclid: 'g_persist',
        fbclid: 'fb_persist',
        utm_source: 'meta',
      });
      expect(mergedAttribution('?utm_source=google&wbraid=wb_new')).toEqual({
        gclid: 'g_persist',
        fbclid: 'fb_persist',
        wbraid: 'wb_new',
        utm_source: 'google',
      });
      expect(
        appendAttributionToUrl(
          'https://sylphxai.github.io/anymd/pricing?entry_surface=seo_verifiable-video-and-cite-check-ai-agents',
          { gclid: 'g_persist', entry_surface: 'older_surface_ignored' }
        )
      ).toBe(
        'https://sylphxai.github.io/anymd/pricing?entry_surface=seo_verifiable-video-and-cite-check-ai-agents&gclid=g_persist'
      );
    } finally {
      Object.defineProperty(globalThis, 'sessionStorage', {
        configurable: true,
        value: prevStorage,
      });
    }
  });
});
