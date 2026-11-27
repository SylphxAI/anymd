import { type Currency, defaultCurrency, formatPrice, isCurrency, proAmount } from './catalog';
import { knownEntrySurface } from './entry-surface';
import type { Locale } from './marketing-locale';
import { safeNext } from './safe-next';

export type PurchaseResume = { currency: Currency; price: string; pricing: string };

// Describes the Pro purchase a signed-out buyer started. The return target is
// the only input: it must be the exact pricing page with canonical,
// allowlisted keys, each at most once. The price comes from the catalogue.
export function purchaseResume(next: string, locale: Locale): PurchaseResume | undefined {
  if (safeNext(next) !== next) return;
  const url = new URL(next, 'https://anymd.invalid');
  if ((url.pathname !== '/pricing' && url.pathname !== '/zh-TW/pricing') || url.hash) return;
  const query = url.search.slice(1);
  if (url.searchParams.toString() !== query) return;
  const seen = new Set<string>();
  for (const [key, value] of url.searchParams) {
    if (seen.has(key) || !(key === 'currency' ? isCurrency(value) : key === 'entry_surface' && knownEntrySurface(value))) return;
    seen.add(key);
  }
  const selected = url.searchParams.get('currency');
  const currency = isCurrency(selected) ? selected : defaultCurrency;
  return { currency, price: formatPrice(proAmount(currency), currency, locale), pricing: next };
}
