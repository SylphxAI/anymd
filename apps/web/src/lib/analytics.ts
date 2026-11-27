import type { SignalConsent } from '@sylphx/sdk/signal';
import type { checkoutParameters } from './funnel';
import { knownEntrySurface, knownProCtaSurface, type EntrySurface, type ProCtaSurface } from './entry-surface';
import { fullAdConsent, googleAdsConfig, type PurchaseConversion } from './google-ads';
import type { SignupSignal } from './signup';
import { localeFromPath, localizedPath } from './marketing-locale';

export const measurementId = 'G-R4TD2Z8RR7';
export const gaDisable = `ga-disable-${measurementId}` as const;

type GoogleWindow = Window & { [gaDisable]?: boolean; dataLayer?: unknown[]; gtag?: (...args: unknown[]) => void };
const allowedSections = new Set(['studio', 'docs', 'vs', 'pro', 'pricing', 'privacy', 'playground', 'legal', 'guide', 'tasks', 'security', 'use-cases', 'vision', 'sign-in', 'account', 'billing', 'dashboard', 'recover', 'cli-auth']);

export function analyticsPath(pathname: string) {
  // Only known translated routes are allowed, not every child of a locale.
  if (localeFromPath(pathname) === 'zh-TW') return localizedPath(pathname, 'zh-TW');
  const path = pathname.split(/[?#]/, 1)[0];
  const section = path.split('/')[1];
  return path === '/' ? '/' : allowedSections.has(section) ? '/' + section : '/other';
}

export function pageLocation(pathname: string) {
  // Never send arbitrary path components, query strings or hashes. Auth and
  // payment returns can include one-time proofs in the URL.
  return location.origin + analyticsPath(pathname);
}

export function isPricingPage(pathname: string) {
  const path = pathname.split(/[?#]/, 1)[0].replace(/\/$/, '');
  return path === '/pricing' || path === '/pro' || analyticsPath(path) === '/zh-TW/pricing';
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
  // Suppress bootstrap page views before 'js', as well as for this config.
  // The live tag can emit one before processing the destination-level setting.
  w.gtag('set', {page_location: pageLocation(pathname), page_referrer: '', page_title: 'Anymd', allow_ad_personalization_signals: false, ads_data_redaction: true, url_passthrough: false, send_page_view: false});
  w.gtag('js', new Date());
  // The app owns page_view (with a redacted URL). The GA4 stream must also
  // have Enhanced Measurement’s browser history page views disabled;
  // send_page_view:false does not disable the stream's history listener.
  w.gtag('config', measurementId, {send_page_view: false, allow_google_signals: consent.ad_storage === 'granted', allow_ad_personalization_signals: false});
  const tag = document.createElement('script');
  tag.id = 'anymd-google-tag';
  tag.async = true;
  tag.referrerPolicy = 'no-referrer';
  tag.src = 'https://www.googletagmanager.com/gtag/js?id=' + measurementId;
  document.head.append(tag);
}

// The provider passes effective Signal consent only after a stored choice.
export function configureGoogleAds(consent: SignalConsent | undefined, gpc: boolean) {
  const {googleAdsId, googleAdsPurchaseLabel} = document.documentElement.dataset;
  const ads = googleAdsConfig(googleAdsId, googleAdsPurchaseLabel);
  const w = google();
  if (!ads || !fullAdConsent(consent, gpc) || w[gaDisable] || !w.gtag) return;
  w.gtag('config', ads.googleAdsId, {send_page_view:false});
  return ads;
}

export function updateGoogleConsent(consent: SignalConsent) {
  google().gtag?.('consent', 'update', consent);
}

export function trackPurchaseConversion(ads: NonNullable<ReturnType<typeof googleAdsConfig>>, purchase: PurchaseConversion) {
  const w = google();
  if (w[gaDisable] || !w.gtag) return false;
  w.gtag('event', 'conversion', {send_to:ads.googleAdsId + '/' + ads.googleAdsPurchaseLabel,
    value:purchase.value, currency:purchase.currency, transaction_id:purchase.transactionId,
    page_location:pageLocation('/pro/thanks'), page_referrer:'', page_title:'Anymd'});
  return true;
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
  return emit(name, pathname, parameters);
}

function emit(name: string, pathname: string, parameters?: object) {
  const w = google();
  if (w[gaDisable] || typeof w.gtag !== 'function') return false;
  const context = {page_location: pageLocation(pathname), page_referrer: '', page_title: 'Anymd'};
  w.gtag('set', context);
  w.gtag('event', name, {...parameters, ...context, send_to: measurementId});
  return true;
}

const fileFormats = new Set(['pdf', 'docx', 'pptx', 'xlsx', 'csv', 'tsv', 'epub', 'html', 'image', 'video', 'text', 'subtitles']);
export type ConversionSource = 'sample' | 'local_file';
type Conversion = { file_format: string; source_kind: ConversionSource };
export type CopyDetails =
  | { surface: 'studio_output' | 'studio_timestamp' | 'home_demo' | 'markdownify_mcp_demo' | 'docs_code' }
  | { surface: 'home_install'; command_kind: 'npm' | 'pip' | 'uvx' }
  | { surface: 'dashboard_mcp'; command_kind: 'mcp_config' }
  | { surface: 'dashboard_cli'; command_kind: 'cli_login' }
  | { surface: 'markdownify_mcp'; command_kind: 'convert_command' | 'cli_install' | 'mcp_config' | 'uvx' };

export type BehavioralEvent =
  | [name: 'document_converted', payload: Conversion & { unit_count: number; token_count: number; duration_ms: number }]
  | [name: 'output_copied', payload: CopyDetails]
  | [name: 'output_downloaded', payload: Conversion]
  | [name: 'quote_searched', payload: { matched: boolean }]
  | [name: 'paywall_view', payload: { entry_surface: 'studio_cite_preview' }]
  | [name: 'pro_cta_clicked', payload: { entry_surface: ProCtaSurface }]
  // A signed-out click on the real Buy control: intent only, not checkout or payment.
  | [name: 'pricing_buy_clicked', payload: { entry_surface: EntrySurface }]
  | [name: 'cli_authorized', payload: { entry_surface: 'cli_auth' }];

const count = (value: number) => Number.isSafeInteger(value) && value >= 0 ? value : 0;
const conversion = (payload: Conversion) => ({
  file_format: fileFormats.has(payload.file_format) ? payload.file_format : 'other',
  source_kind: payload.source_kind === 'local_file' ? 'local_file' : 'sample',
});

export function behavioralParameters(...[name, payload]: BehavioralEvent) {
  // Project fields instead of spreading callers' data: structural typing alone
  // doesn't exclude extra properties. Never forward the converter result itself.
  switch (name) {
    case 'document_converted': return {...conversion(payload), unit_count:count(payload.unit_count), token_count:count(payload.token_count), duration_ms:count(payload.duration_ms)};
    case 'output_downloaded': return conversion(payload);
    case 'output_copied': {
      switch (payload.surface) {
        case 'studio_output': case 'studio_timestamp': case 'home_demo': case 'markdownify_mcp_demo': case 'docs_code': return {surface:payload.surface};
        case 'home_install': return {surface:'home_install', command_kind:['npm', 'pip', 'uvx'].find(kind => kind === payload.command_kind)};
        case 'dashboard_mcp': return {surface:'dashboard_mcp', command_kind:'mcp_config'};
        case 'markdownify_mcp': {
          const kind = ['convert_command', 'cli_install', 'mcp_config', 'uvx'].find(value => value === payload.command_kind);
          return kind ? {surface:payload.surface, command_kind:kind} : undefined;
        }
        case 'dashboard_cli': return {surface:'dashboard_cli', command_kind:'cli_login'};
      }
      return;
    }
    case 'quote_searched': return {matched:payload.matched === true};
    case 'paywall_view': return {entry_surface:'studio_cite_preview'};
    case 'pro_cta_clicked': {
      // Share the product-owned registry with checkout attribution.
      const surface = knownProCtaSurface(payload.entry_surface);
      return surface ? {entry_surface:surface} : undefined;
    }
    case 'pricing_buy_clicked': {
      const surface = knownEntrySurface(payload.entry_surface);
      return surface ? {entry_surface:surface} : undefined;
    }
    case 'cli_authorized': return {entry_surface:'cli_auth'};
  }
}

// Existing GA4 transport; Signal uses the same allowlisted projection.
export function trackBehavioral(consent: SignalConsent | undefined, pathname: string, ...event: BehavioralEvent) {
  if (consent?.analytics_storage !== 'granted') return;
  const parameters = behavioralParameters(...event);
  if (!parameters) return;
  try { emit(event[0], pathname, parameters); } catch { /* Telemetry never blocks the action. */ }
}
