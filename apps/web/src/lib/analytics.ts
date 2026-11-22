import { consentFor } from './consent';

export const measurementId = 'G-R4TD2Z8RR7';
export const gaDisable = 'ga-disable-G-R4TD2Z8RR7';
export type GoogleConsent = ReturnType<typeof consentFor>;
export type Purchase = {transaction_id: string; currency: string; value: number};

type GoogleWindow = Window & { [gaDisable]?: boolean; dataLayer?: unknown[]; gtag?: (...args: unknown[]) => void };
const allowedSections = new Set(['studio', 'docs', 'vs', 'pro', 'pricing', 'privacy', 'playground', 'legal', 'guide', 'tasks', 'security', 'use-cases', 'vision', 'sign-in', 'account', 'billing', 'dashboard', 'recover', 'cli-auth']);

export function pageLocation(pathname: string) {
  // GA sees only a section, never arbitrary path components, query strings or
  // hashes. Auth and payment returns can include one-time proofs in the URL.
  const section = pathname.split('/')[1];
  const path = pathname === '/' ? '/' : allowedSections.has(section) ? '/' + section : '/other';
  return location.origin + path;
}

function google() { return window as GoogleWindow; }

export function startGoogle(consent: GoogleConsent, pathname: string) {
  const w = google();
  w[gaDisable] = false;
  w.dataLayer = [];
  w.gtag = function () { w.dataLayer?.push(arguments); };
  // No tag exists in the denied state. Set the full policy before configuring
  // the tag; do not send an 'update' ping on withdrawal.
  w.gtag('consent', 'default', consent);
  w.gtag('set', {page_location: pageLocation(pathname), page_referrer: '', page_title: 'Anymd', allow_ad_personalization_signals: false, ads_data_redaction: true, url_passthrough: false});
  w.gtag('js', new Date());
  w.gtag('config', measurementId, {send_page_view: false, allow_google_signals: consent.ad_storage === 'granted', allow_ad_personalization_signals: false});
  const tag = document.createElement('script');
  tag.id = 'anymd-google-tag';
  tag.async = true;
  tag.referrerPolicy = 'no-referrer';
  tag.src = 'https://www.googletagmanager.com/gtag/js?id=' + measurementId;
  document.head.append(tag);
}

export function stopGoogle() {
  const w = google();
  w[gaDisable] = true;
  document.getElementById('anymd-google-tag')?.remove();
  w.gtag = undefined;
  if (w.dataLayer) w.dataLayer.length = 0;
}

export function clearGoogleCookies(analytics: boolean) {
  const names = document.cookie.split(';').map(cookie => cookie.split('=', 1)[0].trim());
  const labels = location.hostname.split('.');
  const domains = ['', ...labels.map((_, i) => '; Domain=' + labels.slice(i).join('.'))];
  for (const name of names) {
    if (!/^_gcl_[\w-]+$/.test(name) && !(analytics && /^(?:_ga(?:_[\w-]+)?|_gid|_gat(?:_[\w-]+)?)$/.test(name))) continue;
    for (const domain of domains) document.cookie = name + '=; Max-Age=0; Path=/' + domain;
  }
}

export function track(name: string, pathname: string, parameters: Record<string, unknown> = {}) {
  const w = google();
  const context = {page_location: pageLocation(pathname), page_referrer: '', page_title: 'Anymd'};
  if (w[gaDisable] || !w.gtag) return;
  w.gtag('set', context);
  w.gtag('event', name, {...parameters, ...context, send_to: measurementId});
}

const checkoutPrefix = 'anymd.ga4.checkout.';

export function checkoutContext(processorSessionId: string, checkoutId?: string | null) {
  const key = checkoutPrefix + processorSessionId;
  try {
    if (checkoutId === null) sessionStorage.removeItem(key);
    else if (checkoutId !== undefined) sessionStorage.setItem(key, checkoutId);
    else return sessionStorage.getItem(key);
  } catch { /* optional measurement must not affect checkout */ }
  return null;
}

export function clearCheckoutContexts() {
  try {
    for (const key of Object.keys(sessionStorage)) if (key.startsWith(checkoutPrefix)) sessionStorage.removeItem(key);
  } catch { /* storage unavailable */ }
}
