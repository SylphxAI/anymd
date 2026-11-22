import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseCapabilities } from './auth-capabilities';
import { safeNext } from './safe-next';
import { completeCode, maskEmail, normalizeCode, signInAlias, signInEntry } from './sign-in-flow';

test('tenant capabilities select email OTP, passkey, or no method without a lookup', () => {
  assert.equal(signInEntry(parseCapabilities({ methods: { email_otp: false, passkey: false, password: true } }).methods), 'unavailable');
  assert.equal(signInEntry(parseCapabilities({ methods: { email_otp: true, passkey: true, password: true } }).methods), 'email_otp');
  assert.equal(signInEntry(parseCapabilities({ methods: { passkey: true } }).methods), 'other');
  assert.equal(signInEntry(parseCapabilities({ methods: { sign_up: true } }).methods), 'unavailable');
});

test('OTP input accepts pasted digits without revealing the full recipient', () => {
  assert.equal(normalizeCode('012 345\n'), '012345');
  assert(!completeCode.test(normalizeCode('12a34')));
  assert(completeCode.test(normalizeCode('0 12-345')));
  assert.equal(normalizeCode('123456789'), '123456');
  assert.equal(maskEmail('person@example.com'), 'p•••@example.com');
});

test('legacy sign-in aliases cannot become post-auth redirect loops', () => {
  for (const route of ['/login', '/signup/', '/sign-up', '/REGISTER', '/forgot-password']) {
    assert(signInAlias(route));
    assert.equal(safeNext(route + '?next=/billing'), '/dashboard');
  }
  assert.equal(safeNext('/cli-auth?state=example#top'), '/cli-auth?state=example#top');
  assert.equal(safeNext('//example.invalid'), '/dashboard');
});
