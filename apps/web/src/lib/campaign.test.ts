import assert from 'node:assert/strict';
import { test } from 'node:test';
import { carryCampaign } from './campaign';

test('preserves destination encoding, currency and return path without forwarding unrelated parameters', () => {
  assert.equal(carryCampaign('/sign-in?next=%2Fpricing%3Fcurrency%3DEUR#main', '?utm_campaign=PDF+%26+video&gclid=Test_42&session_id=private'),
    '/sign-in?next=%2Fpricing%3Fcurrency%3DEUR&gclid=Test_42&utm_campaign=PDF+%26+video#main');
});

test('campaign links do not decorate off-site or API destinations', () => {
  for (const href of ['https://other.invalid/pricing', '//other.invalid/pricing', '/\\other.invalid', '/api/auth/google', '#main']) {
    assert.equal(carryCampaign(href, '?gclid=Test_42'), href);
  }
});

test('skips ambiguous or malformed values and does not override destination attribution', () => {
  assert.equal(carryCampaign('/pricing?utm_source=new', '?utm_source=old&gbraid=bad+space&wbraid=Good-3&gclid=A&gclid=B&utm_term=%00&utm_content=Release+notes'),
    '/pricing?utm_source=new&wbraid=Good-3&utm_content=Release+notes');
});
