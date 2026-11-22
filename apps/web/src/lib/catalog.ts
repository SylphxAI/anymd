import catalogue from '../../../../config/money-catalogue.json';
import { consentText } from './money-public.generated';

export { consentText };
export const product = catalogue.spec.products[0];
export const price = product.prices[0];
export const tokenProduct = product.licence.token_product;
const symbols: Record<string, string> = { USD: '$', HKD: 'HK$', TWD: 'NT$', GBP: '£', EUR: '€', JPY: '¥', KRW: '₩', MYR: 'RM', SGD: 'S$', CAD: 'CA$', AUD: 'A$', NZD: 'NZ$', THB: '฿', PHP: '₱', IDR: 'Rp', VND: '₫' };
const zeroDecimals = new Set(['JPY','KRW','VND']);
export const currencyScale = (code: string) => zeroDecimals.has(code) ? 1 : 100;
const number = new Intl.NumberFormat('en-US', { maximumFractionDigits: 2 });
export const currencyOptions = Object.entries(price.unit_amounts).sort(([a], [b]) => a === 'USD' ? -1 : b === 'USD' ? 1 : a.localeCompare(b)).map(([code, amount]) => ({
  code, label: symbols[code] + number.format(Number(amount) / currencyScale(code)),
}));

export const usdPrice = Number(price.unit_amounts.USD) / currencyScale('USD');
export const usdPriceLabel = '$' + number.format(usdPrice);
