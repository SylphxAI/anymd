import assert from 'node:assert/strict';
import { test } from 'node:test';
import { languageSwitchTarget, localeFromPath, pageLinks, pricingReturnPath, pricingSwitchQuery, signInReturnPath } from './marketing-locale';

const origin = 'https://anymd.sylphx.com';

for (const [path, locale, en, tw] of [
  ['/', 'en', '/', '/zh-TW'],
  ['/zh-TW/', 'zh-TW', '/', '/zh-TW'],
  ['/zh-Hant/pricing/?currency=TWD', 'zh-TW', '/pricing', '/zh-TW/pricing'],
  ['/pricing?currency=TWD#plans', 'en', '/pricing', '/zh-TW/pricing'],
  ['/studio?utm_source=fixture#studio-cite-heading', 'en', '/studio', '/zh-TW/studio'],
  ['/zh-TW/studio/', 'zh-TW', '/studio', '/zh-TW/studio'],
  ['/zh-Hant/studio?gclid=fixture', 'zh-TW', '/studio', '/zh-TW/studio'],
  ['/zh-TW//pricing/?currency=TWD', 'zh-TW', '/pricing', '/zh-TW/pricing'],
] as const) {
  test('normalizes ' + path + ' to one reciprocal locale set', () => {
    assert.equal(localeFromPath(path), locale);
    assert.equal(languageSwitchTarget(path, 'en'), en);
    assert.equal(languageSwitchTarget(path, 'zh-TW'), tw);
    assert.deepEqual(pageLinks(path, locale), {
      canonical: origin + (locale === 'en' ? en : tw),
      alternates: [
        { lang: 'en', href: origin + en },
        { lang: 'zh-TW', href: origin + tw },
        { lang: 'x-default', href: origin + en },
      ],
    });
  });
}

test('pricing language switches retain only singleton, allowlisted currency and attribution', () => {
  assert.equal(pricingSwitchQuery('?entry_surface=seo_vs-docling&currency=HKD&proof=private'), 'entry_surface=seo_vs-docling&currency=HKD');
  assert.equal(pricingSwitchQuery('?currency=HKD&entry_surface=task_check_pdf_citation'), 'entry_surface=task_check_pdf_citation&currency=HKD');
  assert.equal(pricingSwitchQuery('?entry_surface=injected_bad&currency=HKD'), 'currency=HKD');
  assert.equal(pricingSwitchQuery('?entry_surface=home_hero&currency=ZZZ'), 'entry_surface=home_hero');
  assert.equal(pricingSwitchQuery('?entry_surface=pricing'), '');
  assert.equal(pricingSwitchQuery('?entry_surface=home_hero&entry_surface=home_hero&currency=HKD&currency=HKD'), '');
});

test('pricing sign-in keeps only singleton, allowlisted checkout values', () => {
  assert.equal(signInReturnPath('/pricing', '?entry_surface=seo_vs-docling&currency=HKD&email=private'), '/pricing?entry_surface=seo_vs-docling&currency=HKD');
  assert.equal(signInReturnPath('/zh-TW/pricing', '?entry_surface=home_hero&currency=HKD'), '/zh-TW/pricing?entry_surface=home_hero&currency=HKD');
  assert.equal(signInReturnPath('/pricing', '?entry_surface=injected_bad&currency=ZZZ&email=private'), '/pricing');
  assert.equal(signInReturnPath('/pricing', '?entry_surface=home_hero&entry%5Fsurface=home_hero&currency=HKD'), '/pricing?currency=HKD');
  assert.equal(signInReturnPath('/pricing', '?entry_surface=home_hero&currency=HKD&currency=HKD'), '/pricing?entry_surface=home_hero');
  assert.equal(signInReturnPath('/zh-TW/pricing', '?entry_surface=injected_bad&currency=ZZZ'), '/zh-TW/pricing?entry_surface=pricing');
  assert.equal(signInReturnPath('/zh-Hant/pricing', ''), '/zh-TW/pricing?entry_surface=pricing');
});

test('non-pricing header returns stay unchanged, including the legacy Pro page', () => {
  const search = '?entry_surface=home_hero&currency=HKD';
  assert.equal(signInReturnPath('/', search), '/dashboard');
  assert.equal(signInReturnPath('/docs', search), '/docs');
  assert.equal(signInReturnPath('/pro', search), '/pro');
  assert.equal(pricingReturnPath('/pro', ''), '/pricing?entry_surface=pro');
});

test('does not advertise untranslated docs or invent a switch target', () => {
  assert.equal(localeFromPath('/zh-TW/docs'), 'en');
  assert.equal(localeFromPath('/zh-TW-foo'), 'en');
  assert.deepEqual(pageLinks('/docs//cli/#install', 'zh-TW'), {
    canonical: origin + '/docs/cli',
    alternates: [{ lang: 'en', href: origin + '/docs/cli' }, { lang: 'x-default', href: origin + '/docs/cli' }],
  });
  assert.equal(languageSwitchTarget('/docs', 'zh-TW'), undefined);
  assert.equal(languageSwitchTarget('/pro', 'zh-TW'), '/zh-TW/pricing');
});
