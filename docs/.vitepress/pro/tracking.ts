/**
 * Pro landing tracking and attribution forwarding.
 * Preserves inbound ad click IDs (gclid, gbraid, wbraid, fbclid) and UTM
 * parameters across /pro, docs navigation, /pricing, and checkout requests.
 * Default-deny fail-closed GA4 / Google Ads tags: zero gtag scripts or
 * beacons fire until explicit analytics consent is granted.
 */
import config from './config.json';

export type Choice = 'all' | 'analytics' | 'none';
export type TrackingPage = 'docs' | 'pro' | 'thanks';
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
export const ATTRIBUTION_KEYS = [...CLICK_ID_KEYS, ...UTM_KEYS, 'entry_surface'] as const;
export type AttributionMap = Record<string, string>;

export const STORE_KEY = 'anymd_pro_consent';
export const ATTRIBUTION_STORAGE_KEY = 'anymd_pro_attribution';
export const PURCHASE_STORAGE_PREFIX = 'anymd_pro_purchase_';
const MAX_ATTRIBUTION_VALUE_LENGTH = 500;
const CLICK_ID_PATTERN = /^[A-Za-z0-9._-]{1,500}$/;
const ALL_DENIED: ConsentState = {
  ad_storage: 'denied',
  ad_user_data: 'denied',
  ad_personalization: 'denied',
  analytics_storage: 'denied',
};

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

export function hasAnalyticsConsent(choice: Choice | null = savedChoice()): boolean {
  return choice === 'all' || choice === 'analytics';
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

/**
 * True for the storefront pages built after VitePress (`/pricing`, `/recover`). The VitePress
 * client router has no route for them and would render its 404, so they must load in full.
 */
export function isStorefrontHref(href: string, origin = 'https://sylphxai.github.io'): boolean {
  try {
    const parsed = new URL(href, `${origin}/anymd/`);
    if (parsed.origin !== origin) return false;
    const path = parsed.pathname.replace(/\/+$/, '').replace(/\.html$/, '');
    return path === '/anymd/pricing' || path === '/anymd/recover';
  } catch {
    return false;
  }
}

export function buyHref(attributionOrClickId: AttributionMap | string = ''): string {
  return appendAttributionToUrl(buyUrl, attributionOrClickId);
}

export function eventDimensions(checkoutTrigger: string, search = typeof location !== 'undefined' ? location.search : ''): Record<string, string> {
  const attr = mergedAttribution(search);
  const path = typeof location !== 'undefined' ? location.pathname : '';
  const fallbackSurface = path.includes('/pro') ? 'docs_pro' : path.includes('/pricing') ? 'pricing_page' : 'docs';
  return {
    entry_surface: attr.entry_surface || fallbackSurface,
    plan: 'pro',
    plan_sku: 'anymd_pro',
    billing_interval: 'one_time',
    option: 'anymd_pro',
    checkout_trigger: checkoutTrigger,
  };
}

let gtag: Gtag | null = null;

/** Test helper only: resets module-level gtag handle. */
export function _resetTrackingForTests(): void {
  gtag = null;
}

function isAnalyticsCookieName(name: string): boolean {
  return name === '_ga' || name.startsWith('_ga_') || name === '_gid' || name.startsWith('_gat');
}

function analyticsCookieNames(): string[] {
  if (typeof document === 'undefined' || typeof document.cookie !== 'string' || !document.cookie) {
    return [];
  }
  const names = new Set<string>();
  for (const part of document.cookie.split(';')) {
    const eq = part.indexOf('=');
    const name = (eq === -1 ? part : part.slice(0, eq)).trim();
    if (name && isAnalyticsCookieName(name)) {
      names.add(name);
    }
  }
  return Array.from(names);
}

function currentHostname(): string {
  if (typeof location === 'undefined') return '';
  if (typeof location.hostname === 'string' && location.hostname) {
    return location.hostname;
  }
  try {
    if (typeof location.origin === 'string' && location.origin && location.origin !== 'null') {
      return new URL(location.origin).hostname;
    }
  } catch {
    // Ignore malformed origins in partial test stubs.
  }
  return '';
}

function cookieDomains(hostname: string): string[] {
  const domains = new Set<string>(['']);
  const host = hostname.trim();
  if (!host) return Array.from(domains);
  domains.add(host);
  if (host.includes('.')) domains.add(`.${host}`);
  const isIp = /^\d+(?:\.\d+){3}$/.test(host) || host.includes(':');
  if (!isIp) {
    const parts = host.split('.').filter(Boolean);
    for (let i = 1; i < parts.length - 1; i++) {
      const suffix = parts.slice(i).join('.');
      domains.add(suffix);
      domains.add(`.${suffix}`);
    }
  }
  return Array.from(domains);
}

function cookiePaths(): string[] {
  const paths = new Set<string>(['/', '/anymd', '/anymd/']);
  if (typeof location !== 'undefined' && typeof location.pathname === 'string') {
    const path = location.pathname;
    if (path.startsWith('/')) {
      paths.add(path);
      const dir = path.replace(/\/[^/]*$/, '/');
      if (dir) paths.add(dir);
    }
  }
  return Array.from(paths);
}

function setGaDisable(disabled: boolean): void {
  if (typeof window === 'undefined') return;
  const w = window as unknown as Record<string, unknown>;
  for (const id of [proConfig.ga4MeasurementId, proConfig.adsConversionId]) {
    if (id && !isPlaceholder(id)) {
      w[`ga-disable-${id}`] = disabled;
    }
  }
}

/** Expires Google Analytics cookies across site paths and host/domain variants. */
export function purgeAnalyticsCookies(): void {
  const names = analyticsCookieNames();
  if (names.length === 0 || typeof document === 'undefined') return;
  const domains = cookieDomains(currentHostname());
  const paths = cookiePaths();
  const expired = 'expires=Thu, 01 Jan 1970 00:00:00 GMT; max-age=0';
  for (const name of names) {
    for (const path of paths) {
      for (const domain of domains) {
        const domainAttr = domain ? `; domain=${domain}` : '';
        document.cookie = `${name}=; ${expired}; path=${path}${domainAttr}`;
      }
    }
  }
}

function sanitizedPageLocation(): string | undefined {
  if (typeof location === 'undefined') return undefined;
  if (location.search.includes('session_id=') || location.search.includes('recovery_token=')) {
    return location.origin + location.pathname;
  }
  return undefined;
}

/** Loads gtag strictly after opt-in consent. Fail-closed: zero scripts or beacons pre-consent. */
export function loadTags(page: TrackingPage = 'docs'): boolean {
  const choice = savedChoice();
  if (!trackingEnabled() || !hasAnalyticsConsent(choice)) {
    if (choice === 'none' || !trackingEnabled()) {
      setGaDisable(true);
      purgeAnalyticsCookies();
    }
    return false;
  }
  setGaDisable(false);
  if (gtag) return true;
  if (typeof window === 'undefined' || typeof document === 'undefined') return false;
  const ga4 = proConfig.ga4MeasurementId ?? '';
  const ads = proConfig.adsConversionId ?? '';
  const useGa = !isPlaceholder(ga4);
  const w = window as unknown as { dataLayer: unknown[]; gtag?: Gtag };
  w.dataLayer = w.dataLayer || [];
  const g: Gtag = function () {
    // gtag.js reads the arguments object, not an array.
    // biome-ignore lint/complexity/noArguments: required by the gtag.js queue protocol
    w.dataLayer.push(arguments);
  };
  gtag = g;
  w.gtag = g;
  g('consent', 'default', { ...ALL_DENIED, wait_for_update: 500 });
  g('consent', 'update', consentState(choice as Choice));
  g('set', 'ads_data_redaction', true);
  g('js', new Date());
  const safeLoc = page === 'thanks' ? (location.origin + location.pathname) : sanitizedPageLocation();
  const cfgParams: Record<string, unknown> = { send_page_view: false };
  if (safeLoc) cfgParams.page_location = safeLoc;
  if (useGa && ga4) {
    g('config', ga4, cfgParams);
    g('event', 'page_view', {
      send_to: ga4,
      ...eventDimensions('page_view'),
      ...(safeLoc ? { page_location: safeLoc } : {}),
    });
  }
  if (!isPlaceholder(ads) && ads) {
    g('config', ads, safeLoc ? { page_location: safeLoc } : {});
  }
  const tagId = encodeURIComponent(useGa ? ga4 : ads);
  const src = `https://www.googletagmanager.com/gtag/js?id=${tagId}`;
  if (!document.querySelector(`script[src="${src}"]`)) {
    const s = document.createElement('script');
    s.async = true;
    s.src = src;
    document.head.appendChild(s);
  }
  return true;
}

export function trackPageView(pageLocation?: string): void {
  if (!gtag || !hasAnalyticsConsent()) return;
  const payload: Record<string, unknown> = {
    send_to: proConfig.ga4MeasurementId,
    ...eventDimensions('page_view'),
  };
  const safeLoc = pageLocation ?? sanitizedPageLocation();
  if (safeLoc) payload.page_location = safeLoc;
  gtag('event', 'page_view', payload);
}

export function updateConsent(choice: Choice, page: TrackingPage = 'docs'): void {
  saveChoice(choice);
  if (hasAnalyticsConsent(choice)) {
    setGaDisable(false);
    if (!gtag) {
      loadTags(page);
    } else {
      gtag('consent', 'update', consentState(choice));
    }
    return;
  }
  setGaDisable(true);
  gtag?.('consent', 'update', ALL_DENIED);
  purgeAnalyticsCookies();
}

export function trackBeginCheckout(checkoutTrigger = 'pro_buy_cta'): void {
  if (!gtag || !hasAnalyticsConsent()) return;
  gtag('event', 'begin_checkout', {
    currency: 'USD',
    value: PRO_VALUE,
    ...eventDimensions(checkoutTrigger),
    items: [{ item_id: 'anymd_pro', item_name: 'anymd Pro', price: PRO_VALUE, quantity: 1 }],
    transport_type: 'beacon',
  });
}

export function transactionId(search: string): string {
  const id = new URLSearchParams(search).get('session_id') ?? '';
  return /^cs_[A-Za-z0-9_]+$/.test(id) ? id : '';
}

export function trackPurchase(search: string): string {
  const id = transactionId(search);
  if (!id) return '';
  if (!gtag || !hasAnalyticsConsent()) return id;
  const key = `${PURCHASE_STORAGE_PREFIX}${id}`;
  if (storageGet('session', key)) return id;
  storageSet('session', key, '1');
  const base = { transaction_id: id, value: PRO_VALUE, currency: 'USD' };
  gtag('event', 'purchase', {
    ...base,
    ...eventDimensions('stripe_redirect', search),
    items: [{ item_id: 'anymd_pro', price: PRO_VALUE, quantity: 1 }],
  });
  if (!isPlaceholder(proConfig.purchaseSendTo) && proConfig.purchaseSendTo) {
    gtag('event', 'conversion', { ...base, send_to: proConfig.purchaseSendTo });
  }
  return id;
}
