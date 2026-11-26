import { canonicalOrigin } from './app-origin';
import { isCurrency } from './catalog';
import { checkoutSurface } from './funnel';
import { safeNext } from './safe-next';

export const locales = ['en', 'zh-TW'] as const;
export type Locale = (typeof locales)[number];
export const translatedPaths = { '/': '/zh-TW', '/pricing': '/zh-TW/pricing', '/studio': '/zh-TW/studio', '/sign-in': '/zh-TW/sign-in' } as const;
type TranslatedPath = keyof typeof translatedPaths;
const isTranslated = (path: string): path is TranslatedPath => Object.hasOwn(translatedPaths, path);

// Head links describe equivalent, indexable content, not request-specific
// currency, campaign or recovery parameters.
export function normalizedPath(path: string) {
  return '/' + (path.split(/[?#]/, 1)[0] ?? '').split('/').filter(Boolean).join('/');
}

export function localeFromPath(path: string): Locale {
  const clean = normalizedPath(path);
  const [, prefix, ...parts] = clean.split('/');
  return /^(?:zh-tw|zh-hant)$/i.test(prefix ?? '') && isTranslated('/' + parts.join('/')) ? 'zh-TW' : 'en';
}

export function localizedPath(path: string, locale: Locale) {
  const clean = normalizedPath(path).replace(/^\/(?:zh-tw|zh-hant)(?=\/|$)/i, '') || '/';
  return locale === 'zh-TW' && isTranslated(clean) ? translatedPaths[clean] : clean;
}

export function pageLinks(path: string, locale: Locale) {
  const en = localizedPath(path, 'en');
  return {
    canonical: canonicalOrigin + localizedPath(en, locale),
    alternates: [
      ...(isTranslated(en) ? locales : ['en'] as const).map(lang => ({ lang, href: canonicalOrigin + localizedPath(en, lang) })),
      { lang: 'x-default', href: canonicalOrigin + en },
    ],
  };
}

export function languageSwitchTarget(path: string, locale: Locale) {
  const en = localizedPath(path, 'en');
  const page = en === '/pro' ? '/pricing' : en;
  return isTranslated(page) ? (locale === 'zh-TW' ? translatedPaths[page] : page) : undefined;
}

function pricingQuery(pathname: string, search: string, includeDefaultSurface = false) {
  const params = new URLSearchParams();
  const surface = checkoutSurface(pathname, search);
  if (includeDefaultSurface || surface !== 'pricing') params.set('entry_surface', surface);
  const currencies = new URLSearchParams(search).getAll('currency');
  if (currencies.length === 1 && isCurrency(currencies[0])) params.set('currency', currencies[0]);
  return params.toString();
}

export function pricingSwitchQuery(search: string) {
  return pricingQuery('/pricing', search);
}

export function signInPath(locale: Locale) {
  return locale === 'zh-TW' ? translatedPaths['/sign-in'] : '/sign-in' as const;
}

export function languageSwitchQuery(pathname: string, search: string) {
  const target = languageSwitchTarget(pathname, 'en');
  if (target === '/sign-in' && new URLSearchParams(search).has('next')) {
    return 'next=' + encodeURIComponent(signInReturnPath(pathname, search));
  }
  return target === '/pricing' ? pricingSwitchQuery(search) : '';
}

export function pricingReturnPath(pathname: string, search: string) {
  const locale = localeFromPath(pathname);
  const path = localizedPath('/pricing', locale);
  // Auth recognizes the localized return as a pricing signup via this surface.
  const query = pricingQuery(pathname, search, locale === 'zh-TW');
  return query ? path + '?' + query : path;
}

export function signInReturnPath(pathname: string, search: string) {
  const page = localizedPath(pathname, 'en');
  if (page === '/sign-in') {
    const values = new URLSearchParams(search).getAll('next');
    return safeNext(values.length === 1 ? values[0] : undefined);
  }
  if (page === '/pricing') return pricingReturnPath(pathname, search);
  return pathname === '/' ? '/dashboard' : pathname;
}
