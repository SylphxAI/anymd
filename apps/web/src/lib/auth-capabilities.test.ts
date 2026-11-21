import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseCapabilities } from './auth-capabilities';

test('reads only supported providers with credentials and explicitly enabled methods', () => {
  assert.deepEqual(parseCapabilities({
    social: [null, { provider: 'google', credentials: 'shared' }, { provider: 'github', credentials: '' }, { provider: 'apple', credentials: 'shared' }],
    methods: { email_otp: false, passkey: 'true', password: true, sign_up: true },
  }), { providers: ['google'], methods: { emailOtp: false, passkey: false, password: true, signUp: true } });
});

test('keeps enabled passkey and email OTP but fails closed for omitted flags', () => {
  assert.deepEqual(parseCapabilities({ methods: { email_otp: true, passkey: true } }), {
    providers: [], methods: { emailOtp: true, passkey: true, password: false, signUp: false },
  });
  assert.deepEqual(parseCapabilities(), { providers: [], methods: { emailOtp: false, passkey: false, password: false, signUp: false } });
});
