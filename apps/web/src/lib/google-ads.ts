import type { SignalConsent } from '@sylphx/sdk/signal';

export function googleAdsConfig(id: unknown, label: unknown) {
  if (typeof id !== 'string' || id !== id.trim() || !/^AW-[0-9]{6,15}$/.test(id) ||
      typeof label !== 'string' || label !== label.trim() || !/^[A-Za-z0-9_-]{1,64}$/.test(label)) return;
  return {googleAdsId:id, googleAdsPurchaseLabel:label};
}

export function fullAdConsent(consent: SignalConsent | undefined, gpc = false) {
  return !gpc && consent?.ad_storage === 'granted' && consent.ad_user_data === 'granted';
}

export const liveCheckoutSession = (value: unknown): value is string =>
  typeof value === 'string' && value === value.trim() && /^cs_live_[A-Za-z0-9_-]{1,124}$/.test(value);

export type PurchaseConversion = {transactionId: string; value: number; currency: string};
