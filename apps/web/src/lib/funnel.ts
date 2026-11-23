import { isCurrency, price, proAmount } from './catalog';
import { knownEntrySurface, type EntrySurface } from './entry-surface';

export function checkoutSurface(pathname: string, search: string) {
  const values = new URLSearchParams(search).getAll('entry_surface');
  return (values.length === 1 && knownEntrySurface(values[0])) || (pathname === '/pro' ? 'pro' : 'pricing');
}

export function checkoutParameters(currency: string, entrySurface: EntrySurface) {
  if (!isCurrency(currency)) return;
  const value = proAmount(currency);
  if (!Number.isFinite(value) || value <= 0) return;
  return {value, currency, plan_sku:price.key, entry_surface:entrySurface, items:[{item_id:price.key, item_name:'Anymd Pro', quantity:1, price:value}]};
}
