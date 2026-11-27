import { isCurrency, price, proAmount } from './catalog';
import { knownEntrySurface, type EntrySurface } from './entry-surface';

export function checkoutSurface(pathname: string, search: string) {
  const values = new URLSearchParams(search).getAll('entry_surface');
  return (values.length === 1 && knownEntrySurface(values[0])) || (pathname === '/pro' ? 'pro' : 'pricing');
}

// /pro is an old alias of /pricing. The target gives checkoutSurface the same
// answer it gave on /pro: one known entry_surface stays, and anything else
// becomes the single value "pro". Other parameters keep their raw bytes and order.
export function proPricingTarget(search: string) {
  const values = new URLSearchParams(search).getAll('entry_surface');
  if (values.length === 1 && knownEntrySurface(values[0])) return '/pricing' + search;
  const rest = search.slice(1).split('&').filter(part => part && !new URLSearchParams(part).has('entry_surface'));
  return '/pricing?' + [...rest, 'entry_surface=pro'].join('&');
}

export function checkoutParameters(currency: string, entrySurface: EntrySurface) {
  if (!isCurrency(currency)) return;
  const value = proAmount(currency);
  if (!Number.isFinite(value) || value <= 0) return;
  return {value, currency, plan_sku:price.key, entry_surface:entrySurface, items:[{item_id:price.key, item_name:'Anymd Pro', quantity:1, price:value}]};
}
