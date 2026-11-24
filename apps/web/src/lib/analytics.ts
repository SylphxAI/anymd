import type { SignalConsent } from '@sylphx/sdk/signal';
import type { checkoutParameters } from './funnel';
import type { SignupSignal } from './signup';

export const measurementId = 'G-R4TD2Z8RR7';
export const gaDisable = `ga-disable-${measurementId}` as const;

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

export function startGoogle(consent: SignalConsent, pathname: string) {
  const w = google();
  w[gaDisable] = false;
  w.dataLayer = [];
  w.gtag = function () { w.dataLayer?.push(arguments); };
  // No tag exists in the denied state. Set the full policy before configuring
  // the tag; do not send an 'update' ping on withdrawal.
  w.gtag('consent', 'default', consent);
  w.gtag('set', {page_location: pageLocation(pathname), page_referrer: '', page_title: 'Anymd', allow_ad_personalization_signals: false, ads_data_redaction: true, url_passthrough: false});
  w.gtag('js', new Date());
  // The app owns page_view (with a redacted URL). The GA4 stream must also
  // have Enhanced Measurement’s browser history page views disabled;
  // send_page_view:false only disables the initial config hit.
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
    if (!/^(?:_gcl_[\w-]+|_fbc|_fbp|anymd_attribution)$/.test(name) && !(analytics && /^(?:_ga(?:_[\w-]+)?|_gid|_gat(?:_[\w-]+)?)$/.test(name))) continue;
    for (const domain of domains) document.cookie = name + '=; Max-Age=0; Path=/' + domain;
  }
}

type CheckoutEvent = NonNullable<ReturnType<typeof checkoutParameters>> & {event_callback: () => void; event_timeout: number};

export function track(...[name, pathname, parameters]:
  | [name: 'page_view' | 'pricing_view', pathname: string]
  | [name: 'begin_checkout', pathname: string, parameters: CheckoutEvent]
  | [name: 'sign_up', pathname: string, parameters: SignupSignal]) {
  const w = google();
  if (w[gaDisable] || typeof w.gtag !== 'function') return false;
  const context = {page_location: pageLocation(pathname), page_referrer: '', page_title: 'Anymd'};
  w.gtag('set', context);
  w.gtag('event', name, {...parameters, ...context, send_to: measurementId});
  return true;
}
