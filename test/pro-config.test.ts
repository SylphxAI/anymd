import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import config from '../docs/.vitepress/pro/config.json';
import {
  ATTRIBUTION_STORAGE_KEY,
  appendAttributionToUrl,
  buyHref,
  clickId,
  extractAttribution,
  isPlaceholder,
  isPricingHref,
  mergedAttribution,
  persistAttribution,
  trackingEnabled,
  trackPurchase,
  transactionId,
} from '../docs/.vitepress/pro/tracking';

describe('pro config', () => {
  test('placeholders are recognised, including the payment link placeholder', () => {
    expect(isPlaceholder('https://PRO_PAYMENT_LINK')).toBe(true);
    expect(isPlaceholder('G-XXXX')).toBe(true);
    expect(isPlaceholder('AW-XXXX')).toBe(true);
    expect(isPlaceholder('')).toBe(true);
    expect(isPlaceholder('https://buy.stripe.com/abc')).toBe(false);
  });

  test('dead GA4/Ads placeholder IDs are removed from pro config', () => {
    const raw = readFileSync(
      new URL('../docs/.vitepress/pro/config.json', import.meta.url),
      'utf8'
    );
    expect(raw).not.toContain('G-XXXX');
    expect(raw).not.toContain('AW-XXXX');
    expect(trackingEnabled()).toBe(false);
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
