import { canonicalOrigin } from './app-origin';

export const locales = ['en', 'zh-TW'] as const;
export type Locale = (typeof locales)[number];
const translatedPaths = new Set(['/', '/pricing']);

// Head links describe equivalent, indexable content, not request-specific
// currency, campaign or recovery parameters.
export function normalizedPath(path: string) {
  return '/' + (path.split(/[?#]/, 1)[0] ?? '').split('/').filter(Boolean).join('/');
}

export function localeFromPath(path: string): Locale {
  const clean = normalizedPath(path);
  const [, prefix, ...parts] = clean.split('/');
  return /^(?:zh-tw|zh-hant)$/i.test(prefix ?? '') && translatedPaths.has('/' + parts.join('/')) ? 'zh-TW' : 'en';
}

export function localizedPath(path: string, locale: Locale) {
  const clean = normalizedPath(path).replace(/^\/(?:zh-tw|zh-hant)(?=\/|$)/i, '') || '/';
  return locale === 'zh-TW' && translatedPaths.has(clean) ? '/zh-TW' + (clean === '/' ? '' : clean) : clean;
}

export function pageLinks(path: string, locale: Locale) {
  const en = localizedPath(path, 'en');
  return {
    canonical: canonicalOrigin + localizedPath(en, locale),
    alternates: [
      ...(translatedPaths.has(en) ? locales : ['en'] as const).map(lang => ({ lang, href: canonicalOrigin + localizedPath(en, lang) })),
      { lang: 'x-default', href: canonicalOrigin + en },
    ],
  };
}

export function languageSwitchTarget(path: string, locale: Locale) {
  const en = localizedPath(path, 'en');
  const page = en === '/pro' ? '/pricing' : en;
  return translatedPaths.has(page) ? localizedPath(page, locale) : undefined;
}
