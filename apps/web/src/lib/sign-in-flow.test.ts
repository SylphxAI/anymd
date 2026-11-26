import assert from 'node:assert/strict';
import { test } from 'node:test';
import { safeNext } from './safe-next';
import { completeCode, maskEmail, normalizeCode, signInAlias } from './sign-in-flow';

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
  for (const route of ['/zh-TW/sign-in', '/ZH-tw/sign-in/', '/zh-TW//sign-in', '/zh-Hant/sign-in']) {
    assert.equal(safeNext(route + '?next=/billing'), '/dashboard');
  }
  assert.equal(safeNext('/cli-auth?state=example#top'), '/cli-auth?state=example#top');
  assert.equal(safeNext('/zh-TW/pricing?entry_surface=home_hero&currency=TWD'), '/zh-TW/pricing?entry_surface=home_hero&currency=TWD');
  assert.equal(safeNext('//example.invalid'), '/dashboard');
});
