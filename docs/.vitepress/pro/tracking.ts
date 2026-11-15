/**
 * Pro landing tracking and attribution forwarding.
 * Preserves inbound ad click IDs (gclid, gbraid, wbraid, fbclid) and UTM
 * parameters across /pro, docs navigation, /pricing, and checkout requests.
 * Optional GA4 / Google Ads tags only load when non-placeholder IDs are set.
 */
import config from './config.json';

export type Choice = 'all' | 'analytics' | 'none';
type Gtag = (...args: unknown[]) => void;
type ConsentState = Record<string, string>;
type ProConfig = {
  buyUrl: string;
  ga4MeasurementId?: string;
  adsConversionId?: string;
  purchaseSendTo?: string;
};

const proConfig: ProConfig = config;

export const PRO_VALUE = 29;
export const CLICK_ID_KEYS = ['gclid', 'gbraid', 'wbraid', 'fbclid'] as const;
export const UTM_KEYS = [
  'utm_source',
  'utm_medium',
  'utm_campaign',
  'utm_content',
  'utm_term',
] as const;
export const ATTRIBUTION_KEYS = [...CLICK_ID_KEYS, ...UTM_KEYS] as const;
export type AttributionMap = Record<string, string>;

const STORE_KEY = 'anymd_pro_consent';
export const ATTRIBUTION_STORAGE_KEY = 'anymd_pro_attribution';
const MAX_ATTRIBUTION_VALUE_LENGTH = 500;
const CLICK_ID_PATTERN = /^[A-Za-z0-9._-]{1,500}$/;
const REGIONS = [
  'AT', 'BE', 'BG', 'HR', 'CY', 'CZ', 'DK', 'EE', 'FI', 'FR', 'DE', 'GR', 'HU', 'IE', 'IT', 'LV',
  'LT', 'LU', 'MT', 'NL', 'PL', 'PT', 'RO', 'SK', 'SI', 'ES', 'SE', 'IS', 'LI', 'NO', 'GB', 'CH',
];

export const buyUrl: string = proConfig.buyUrl;

export function isPlaceholder(value: string | undefined): boolean {
  return !value || /X{4}|PRO_PAYMENT_LINK/.test(value);
}

export function trackingEnabled(): boolean {
  return !isPlaceholder(proConfig.ga4MeasurementId) || !isPlaceholder(proConfig.adsConversionId);
}

function storageGet(store: 'local' | 'session', key: string): string | null {
  try {
    return (store === 'local' ? localStorage : sessionStorage).getItem(key);
  } catch {
    return null;
  }
}

function storageSet(store: 'local' | 'session', key: string, value: string): void {
  try {
    (store === 'local' ? localStorage : sessionStorage).setItem(key, value);
  } catch {
    // Storage blocked: the choice simply is not remembered.
  }
}

export function savedChoice(): Choice | null {
  const v = storageGet('local', STORE_KEY);
  return v === 'all' || v === 'analytics' || v === 'none' ? v : null;
}

export function saveChoice(choice: Choice): void {
  storageSet('local', STORE_KEY, choice);
}

export function consentState(choice: Choice): ConsentState {
  const ad = choice === 'all' ? 'granted' : 'denied';
  return {
    ad_storage: ad,
    ad_user_data: ad,
    ad_personalization: ad,
    analytics_storage: choice === 'none' ? 'denied' : 'granted',
  };
}

function sanitizeAttributionEntry(key: string, raw: string | null | undefined): string {
  if (typeof raw !== 'string') return '';
  const trimmed = raw.trim();
  if (!trimmed) return '';
  if ((CLICK_ID_KEYS as readonly string[]).includes(key)) {
    return CLICK_ID_PATTERN.test(trimmed) ? trimmed : '';
  }
  return trimmed.slice(0, MAX_ATTRIBUTION_VALUE_LENGTH);
}

export function extractAttribution(search: string): AttributionMap {
  const out: AttributionMap = {};
  if (!search) return out;
  const params = new URLSearchParams(search);
  for (const key of ATTRIBUTION_KEYS) {
    const clean = sanitizeAttributionEntry(key, params.get(key));
    if (clean) out[key] = clean;
  }
  return out;
}

export function storedAttribution(): AttributionMap {
  const raw = storageGet('session', ATTRIBUTION_STORAGE_KEY);
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    if (!parsed || typeof parsed !== 'object') return {};
    const out: AttributionMap = {};
    for (const key of ATTRIBUTION_KEYS) {
      const val = parsed[key];
      if (typeof val === 'string') {
        const clean = sanitizeAttributionEntry(key, val);
        if (clean) out[key] = clean;
      }
    }
    return out;
  } catch {
    return {};
  }
}

export function mergedAttribution(search = ''): AttributionMap {
  return { ...storedAttribution(), ...extractAttribution(search) };
}

export function persistAttribution(search: string): AttributionMap {
  const merged = mergedAttribution(search);
  if (Object.keys(merged).length > 0) {
    storageSet('session', ATTRIBUTION_STORAGE_KEY, JSON.stringify(merged));
  }
  return merged;
}

/** Validated Google click id from query params, or ''. */
export function clickId(search: string): string {
  return extractAttribution(search).gclid ?? '';
}

function normalizeAttributionInput(input: AttributionMap | string): AttributionMap {
  if (typeof input !== 'string') {
    const out: AttributionMap = {};
    for (const key of ATTRIBUTION_KEYS) {
      const clean = sanitizeAttributionEntry(key, input[key]);
      if (clean) out[key] = clean;
    }
    return out;
  }
  const trimmed = input.trim();
  if (!trimmed) return {};
  if (trimmed.startsWith('?') || trimmed.includes('=')) {
    return extractAttribution(trimmed);
  }
  const clean = sanitizeAttributionEntry('gclid', trimmed);
  return clean ? { gclid: clean } : {};
}

export function appendAttributionToUrl(
  targetUrl: string,
  attribution: AttributionMap | string
): string {
  const attr = normalizeAttributionInput(attribution);
  if (Object.keys(attr).length === 0 || !targetUrl) return targetUrl;
  const isAbsolute = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(targetUrl);
  const parsed = new URL(targetUrl, isAbsolute ? undefined : 'https://sylphxai.github.io');
  if (/^https:\/\/buy\.stripe\.com\//.test(targetUrl)) {
    if (attr.gclid && !parsed.searchParams.has('client_reference_id')) {
      parsed.searchParams.set('client_reference_id', attr.gclid);
    }
    for (const key of UTM_KEYS) {
      const val = attr[key];
      if (val && !parsed.searchParams.has(key)) {
        parsed.searchParams.set(key, val);
      }
    }
    return parsed.toString();
  }
  for (const key of ATTRIBUTION_KEYS) {
    const val = attr[key];
    if (val && !parsed.searchParams.has(key)) {
      parsed.searchParams.set(key, val);
    }
  }
  return isAbsolute ? parsed.toString() : `${parsed.pathname}${parsed.search}${parsed.hash}`;
}

export function isPricingHref(href: string): boolean {
  if (!href) return false;
  try {
    const parsed = new URL(href, 'https://sylphxai.github.io/anymd/');
    const path = parsed.pathname.replace(/\/+$/, '');
    return path === '/pricing' || path === '/anymd/pricing';
  } catch {
    return false;
  }
}

export function buyHref(attributionOrClickId: AttributionMap | string = ''): string {
  return appendAttributionToUrl(buyUrl, attributionOrClickId);
}

let gtag: Gtag | null = null;

/** Loads gtag once. Returns false (and does nothing) while ids are placeholders. */
export function loadTags(page: 'pro' | 'thanks'): boolean {
  if (gtag) return true;
  if (!trackingEnabled()) return false;
  const ga4 = proConfig.ga4MeasurementId ?? '';
  const ads = proConfig.adsConversionId ?? '';
  const useGa = !isPlaceholder(ga4);
  const w = window as unknown as { dataLayer: unknown[] };
  w.dataLayer = w.dataLayer || [];
  const g: Gtag = function () {
    // gtag.js reads the arguments object, not an array.
    // biome-ignore lint/complexity/noArguments: required by the gtag.js queue protocol
    w.dataLayer.push(arguments);
  };
  gtag = g;
  // Defaults go first, before any config call. Region rule first, then the global default.
  g('consent', 'default', { ...consentState('none'), region: REGIONS, wait_for_update: 500 });
  g('consent', 'default', consentState('all'));
  const saved = savedChoice();
  if (saved) g('consent', 'update', consentState(saved));
  g('set', 'ads_data_redaction', true);
  g('js', new Date());
  const params: Record<string, unknown> = { send_page_view: true };
  // The thanks URL carries a Stripe session id: report the path only.
  if (page === 'thanks') params.page_location = location.origin + location.pathname;
  if (useGa && ga4) g('config', ga4, params);
  if (!isPlaceholder(ads) && ads) {
    g('config', ads, page === 'thanks' ? { page_location: params.page_location } : {});
  }
  const s = document.createElement('script');
  s.async = true;
  s.src = `https://www.googletagmanager.com/gtag/js?id=${encodeURIComponent(useGa ? ga4 : ads)}`;
  document.head.appendChild(s);
  return true;
}

export function updateConsent(choice: Choice): void {
  saveChoice(choice);
  gtag?.('consent', 'update', consentState(choice));
}

export function trackBeginCheckout(): void {
  gtag?.('event', 'begin_checkout', {
    currency: 'USD',
    value: PRO_VALUE,
    items: [{ item_id: 'anymd_pro', item_name: 'anymd Pro', price: PRO_VALUE, quantity: 1 }],
    transport_type: 'beacon',
  });
}

/** Stripe session id from the redirect, or '' (a reload or direct visit never counts as a purchase). */
export function transactionId(search: string): string {
  const id = new URLSearchParams(search).get('session_id') ?? '';
  return /^cs_[A-Za-z0-9_]+$/.test(id) ? id : '';
}

/** Fires `purchase` once per Stripe session id per browser session; nothing without one. Returns the id. */
export function trackPurchase(search: string): string {
  const id = transactionId(search);
  if (!id) return '';
  const key = `anymd_pro_purchase_${id}`;
  if (!gtag || storageGet('session', key)) return id;
  storageSet('session', key, '1');
  const base = { transaction_id: id, value: PRO_VALUE, currency: 'USD' };
  gtag('event', 'purchase', {
    ...base,
    items: [{ item_id: 'anymd_pro', price: PRO_VALUE, quantity: 1 }],
  });
  if (!isPlaceholder(proConfig.purchaseSendTo) && proConfig.purchaseSendTo) {
    gtag('event', 'conversion', { ...base, send_to: proConfig.purchaseSendTo });
  }
  return id;
}
