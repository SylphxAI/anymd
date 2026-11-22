import { canonicalOrigin } from './app-origin';
import { explicitAnalytics, parseChoice } from './consent';
import { knownEntrySurface } from './entry-surface';

export const signupCookie = 'anymd_signup';
const methods = ['google', 'github', 'email_otp', 'passkey'] as const;
const directSurfaces = ['sign_in', 'cli_auth', 'recover'] as const;
export type SignupMethod = typeof methods[number];
export type SignupSignal = {method: SignupMethod; entry_surface: NonNullable<ReturnType<typeof knownEntrySurface>> | typeof directSurfaces[number]};

export function signupSignal(isNewUser: unknown, choice: unknown, gpc: boolean, method: SignupMethod, next: string): SignupSignal | undefined {
  if (isNewUser !== true || !explicitAnalytics(parseChoice(choice), gpc)) return;
  const {pathname, searchParams} = new URL(next, canonicalOrigin);
  const values = searchParams.getAll('entry_surface');
  const surface = values.length === 1 && knownEntrySurface(values[0]);
  return {method, entry_surface: surface || (pathname === '/pricing' ? 'pricing' : pathname === '/pro' ? 'pro'
    : pathname === '/cli-auth' ? 'cli_auth' : pathname === '/recover' ? 'recover' : 'sign_in')};
}

export function parseSignupSignal(value: string): SignupSignal | undefined {
  const [rawMethod, rawSurface, ...rest] = value.split('.');
  const method = methods.find(allowed => allowed === rawMethod);
  const entry_surface = knownEntrySurface(rawSurface) ?? directSurfaces.find(allowed => allowed === rawSurface);
  if (method && entry_surface && !rest.length) return {method, entry_surface};
}
