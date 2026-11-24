import { resolveEffectiveConsent } from '@sylphx/sdk/signal';

// Keep the existing cookie for the one-time Auth signup handoff.
export const consentKey = 'anymd.cookie-settings.v1';
export const signalStorage = 'anymd.signal.v1';
export const signalChoiceKey = signalStorage + '.consent';
export const signalChoices = {reject:'denied', analytics:'analytics_only', all:'granted'} as const;
export type Choice = keyof typeof signalChoices;

export function savedSignalChoice(value: unknown) {
  return value === 'reject' || value === 'analytics' || value === 'all' ? signalChoices[value] : undefined;
}

export function explicitAnalytics(choice: unknown, headers?: Headers) {
  return resolveEffectiveConsent({storedChoice:savedSignalChoice(choice)}, headers).analytics_storage === 'granted';
}
