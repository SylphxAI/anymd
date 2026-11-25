import assert from 'node:assert/strict';
import { test } from 'node:test';
import { carryCampaign } from './campaign';

test('preserves destination encoding, currency and return path without forwarding unrelated parameters', () => {
  assert.equal(carryCampaign('/sign-in?next=%2Fpricing%3Fcurrency%3DEUR#main', '?utm_campaign=PDF+%26+video&utm_id=spring-2026&gclid=Test_42&session_id=private'),
    '/sign-in?next=%2Fpricing%3Fcurrency%3DEUR&gclid=Test_42&utm_campaign=PDF+%26+video&utm_id=spring-2026#main');
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

test('keeps the Markdownify pricing entry surface and every allowed campaign field', () => {
  const base = '/pricing?entry_surface=seo_vs-markdownify-mcp';
  const campaign = 'gclid=TEST999&gbraid=A1&wbraid=B2&utm_source=google&utm_medium=cpc&utm_campaign=docs&utm_id=1&utm_term=pdf&utm_content=compare';
  assert.equal(carryCampaign(base, '?' + campaign + '&entry_surface=home_hero&email=private'), base + '&' + campaign);
});
