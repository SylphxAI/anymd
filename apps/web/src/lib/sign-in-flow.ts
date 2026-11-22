import type { Capabilities } from './auth-capabilities';

export const resendDelaySeconds = 30;
export const completeCode = /^[0-9]{6}$/;
const aliases = new Set(['/login', '/signup', '/sign-up', '/register', '/forgot-password']);

export function signInAlias(pathname: string) {
  return aliases.has(pathname.replace(/\/+$/, '').toLowerCase());
}

export function signInEntry(methods: Capabilities['methods']) {
  // Choose by tenant policy, never by whether a particular email has an account.
  if (methods.emailOtp) return 'email_otp';
  if (methods.passkey || methods.password) return 'other';
  return 'unavailable';
}

export function maskEmail(email: string) {
  const at = email.lastIndexOf('@');
  return at > 0 ? email.slice(0, 1) + '•••' + email.slice(at) : '';
}

export function normalizeCode(value: string) {
  return value.replace(/[^0-9]/g, '').slice(0, 6);
}
