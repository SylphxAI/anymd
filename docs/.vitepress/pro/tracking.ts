/**
 * Pro landing tracking: gtag.js (GA4 + Google Ads) with Consent Mode v2.
 * Runs only on /pro and /pro/thanks (the ProTracking component is mounted by
 * those two pages and nowhere else). While an id is a placeholder, nothing loads
 * and no network request is made. Events carry no email, name or free text.
 */
import config from './config.json';

export type Choice = 'all' | 'analytics' | 'none';
type Gtag = (...args: unknown[]) => void;
type ConsentState = Record<string, string>;

export const PRO_VALUE = 29;
const STORE_KEY = 'anymd_pro_consent';
const REGIONS = [
  'AT', 'BE', 'BG', 'HR', 'CY', 'CZ', 'DK', 'EE', 'FI', 'FR', 'DE', 'GR', 'HU', 'IE', 'IT', 'LV',
  'LT', 'LU', 'MT', 'NL', 'PL', 'PT', 'RO', 'SK', 'SI', 'ES', 'SE', 'IS', 'LI', 'NO', 'GB', 'CH',
];

export const buyUrl: string = config.buyUrl;

export function isPlaceholder(value: string | undefined): boolean {
  return !value || /X{4}|PRO_PAYMENT_LINK/.test(value);
}

export function trackingEnabled(): boolean {
  return !isPlaceholder(config.ga4MeasurementId) || !isPlaceholder(config.adsConversionId);
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

/** Stripe client_reference_id accepts letters, digits, dash and underscore, up to 200 chars. */
export function clickId(search: string): string {
  const raw = new URLSearchParams(search).get('gclid') ?? '';
  return /^[A-Za-z0-9_-]{1,200}$/.test(raw) ? raw : '';
}

export function buyHref(gclid: string): string {
  if (!gclid || !/^https:\/\/buy\.stripe\.com\//.test(buyUrl)) return buyUrl;
  const sep = buyUrl.includes('?') ? '&' : '?';
  return `${buyUrl}${sep}client_reference_id=${encodeURIComponent(gclid)}`;
}

let gtag: Gtag | null = null;

/** Loads gtag once. Returns false (and does nothing) while ids are placeholders. */
export function loadTags(page: 'pro' | 'thanks'): boolean {
  if (gtag) return true;
  if (!trackingEnabled()) return false;
  const ga4 = config.ga4MeasurementId;
  const ads = config.adsConversionId;
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
  if (useGa) g('config', ga4, params);
  if (!isPlaceholder(ads)) g('config', ads, page === 'thanks' ? { page_location: params.page_location } : {});
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
  gtag('event', 'purchase', { ...base, items: [{ item_id: 'anymd_pro', price: PRO_VALUE, quantity: 1 }] });
  if (!isPlaceholder(config.purchaseSendTo)) {
    gtag('event', 'conversion', { ...base, send_to: config.purchaseSendTo });
  }
  return id;
}
