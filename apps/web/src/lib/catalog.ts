import catalogue from '../../../../config/money-catalogue.json';
import { consentText } from './money-public.generated';

export { consentText };
export const product = catalogue.spec.products[0];
export const price = product.prices[0];
export const tokenProduct = product.licence.token_product;

export type Currency = keyof typeof price.unit_amounts;
export const defaultCurrency: Currency = 'USD';
export const currencies = [defaultCurrency, ...(Object.keys(price.unit_amounts) as Currency[]).sort()
  .filter(code => code !== defaultCurrency)];

export function isCurrency(value: unknown): value is Currency {
  return typeof value === 'string' && Object.hasOwn(price.unit_amounts, value);
}

const zeroDecimals = new Set<Currency>(['JPY', 'KRW', 'VND']);
export const proAmount = (code: Currency) => Number(price.unit_amounts[code]) / (zeroDecimals.has(code) ? 1 : 100);

// The server and initial hydration use the same English locale. Interactive
// labels switch to the browser's language preferences only after hydration.
export function formatPrice(amount: number, currency: Currency, locales: string | readonly string[] = 'en') {
  return new Intl.NumberFormat(locales, {
    style: 'currency', currency, minimumFractionDigits: 0,
    maximumFractionDigits: zeroDecimals.has(currency) ? 0 : 2,
  }).format(amount);
}
