import { canonicalOrigin } from './app-origin';
import { isCurrency } from './catalog';
import { checkoutSurface } from './funnel';

export const locales = ['en', 'zh-TW'] as const;
export type Locale = (typeof locales)[number];
export const translatedPaths = { '/': '/zh-TW', '/pricing': '/zh-TW/pricing', '/studio': '/zh-TW/studio' } as const;
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

export function pricingSwitchQuery(search: string) {
  const params = new URLSearchParams();
  const surface = checkoutSurface('/pricing', search);
  if (surface !== 'pricing') params.set('entry_surface', surface);
  const currencies = new URLSearchParams(search).getAll('currency');
  if (currencies.length === 1 && isCurrency(currencies[0])) params.set('currency', currencies[0]);
  return params.toString();
}
