import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseSignupSignal, signupSignal } from './signup';

test('only Auth boolean true and an explicit grant without GPC produce a signup signal', () => {
  for (const isNewUser of [undefined, null, false, 'true', 1]) {
    assert.equal(signupSignal(isNewUser, 'analytics', undefined, 'google', '/dashboard'), undefined);
  }
  for (const choice of [undefined, null, 'reject', 'granted']) {
    assert.equal(signupSignal(true, choice, undefined, 'google', '/dashboard'), undefined);
  }
  for (const choice of ['analytics', 'all']) {
    assert.equal(signupSignal(true, choice, new Headers({'Sec-GPC':'0, 1'}), 'email_otp', '/pricing'), undefined);
    assert.deepEqual(signupSignal(true, choice, undefined, 'email_otp', '/pricing?entry_surface=cli_pdf&code=123456#private'),
      {method:'email_otp', entry_surface:'cli_pdf'});
  }
});

test('signup attribution never copies raw redirect input or unrecognized cookie fields', () => {
  assert.deepEqual(signupSignal(true, 'all', undefined, 'google', '/pricing?entry_surface=private%40example.invalid'), {method:'google', entry_surface:'pricing'});
  assert.deepEqual(signupSignal(true, 'all', undefined, 'github', '/pro?entry_surface=cli_pdf&entry_surface=cli_pdf'), {method:'github', entry_surface:'pro'});
  assert.deepEqual(signupSignal(true, 'all', undefined, 'passkey', '/cli-auth?code=private'), {method:'passkey', entry_surface:'cli_auth'});
  assert.deepEqual(parseSignupSignal('passkey.cli_auth'), {method:'passkey', entry_surface:'cli_auth'});
  assert.deepEqual(parseSignupSignal('google.sign_in'), {method:'google', entry_surface:'sign_in'});
  for (const value of ['', 'google', 'Google.pricing', 'google.pricing.private', 'mystery.pricing', 'google.private@example.invalid']) {
    assert.equal(parseSignupSignal(value), undefined);
  }
});
