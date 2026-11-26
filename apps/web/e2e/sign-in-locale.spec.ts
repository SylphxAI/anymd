import { expect, test } from '@playwright/test';

const paths = {
  en: { pricing: '/pricing', signIn: '/sign-in', heading: 'Sign in to Anymd', cta: 'Sign in to buy Pro', switch: '繁體中文', google: 'Continue with Google', email: 'Email address', privacy: 'privacy notice', terms: 'site terms' },
  'zh-TW': { pricing: '/zh-TW/pricing', signIn: '/zh-TW/sign-in', heading: '登入 Anymd', cta: '登入以購買 Pro', switch: 'English', google: '使用 Google 登入', email: '電子郵件地址', privacy: '隱私權聲明（英文）', terms: '網站使用條款（英文）' },
} as const;

for (const locale of ['en', 'zh-TW'] as const) {
  test(locale + ' pricing lands on sign-in and switches language without altering the continuation', async ({ page, request }) => {
    const entry = paths[locale];
    const other = locale === 'en' ? 'zh-TW' : 'en';
    const query = '?entry_surface=home_hero&currency=TWD';
    const suffix = '?next=' + encodeURIComponent(entry.pricing + query);
    await page.goto(entry.pricing + query);
    const cta = page.getByRole('link', { name: entry.cta, exact: true });
    await expect(cta).toHaveAttribute('href', entry.signIn + suffix);
    await cta.click();
    for (const [step, lang] of [locale, other, locale].entries()) {
      const target = paths[lang];
      await expect(page).toHaveURL(target.signIn + suffix);
      await expect(page.locator('html')).toHaveAttribute('lang', lang);
      await expect(page.getByRole('heading', { name: target.heading })).toBeVisible();
      await expect(page.getByRole('link', { name: target.google, exact: true })).toHaveAttribute('href', '/api/auth/google' + suffix);
      await expect(page.getByRole('textbox', { name: target.email, exact: true })).toBeVisible();
      await expect(page.getByRole('link', { name: target.privacy, exact: true })).toHaveAttribute('href', '/legal/privacy');
      await expect(page.getByRole('link', { name: target.terms, exact: true })).toHaveAttribute('href', '/legal/terms');
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
      const switcher = page.getByRole('link', { name: target.switch, exact: true });
      await expect(switcher).toHaveAttribute('href', paths[lang === locale ? other : locale].signIn + suffix);
      if (step < 2) await switcher.click();
    }
    const response = await request.get(entry.signIn + suffix, { maxRedirects: 0 });
    expect(response.status()).toBe(200);
    expect(response.headers()['cache-control']).toBe('private, no-store');
    expect(response.headers()['referrer-policy']).toBe('strict-origin');
    const html = await response.text();
    expect(html).toMatch(new RegExp('<html[^>]* lang="' + locale + '"'));
    expect(html).toMatch(/<meta\b(?=[^>]*name="robots")(?=[^>]*content="noindex")[^>]*>/);
    expect(html).not.toMatch(/<link\b[^>]+rel="(?:canonical|alternate)"/);
  });
}

test('localized sign-in renders app and SDK errors without forwarding unsafe next', async ({ page, request }) => {
  for (const error of ['invalid_code', 'cancelled']) {
    const path = '/zh-TW/sign-in?error=' + error + '&next=%2F%2Fexample.invalid';
    const response = await request.get(path);
    const html = await response.text();
    expect(html).toContain(error === 'invalid_code' ? '無法驗證這組驗證碼' : '登入已取消');
    await page.goto(path);
    await expect(page.getByRole('alert')).toContainText(error === 'invalid_code' ? '無法驗證這組驗證碼' : '登入已取消');
    await expect(page.getByRole('link', { name: '使用 Google 登入' })).toHaveAttribute('href', '/api/auth/google?next=%2Fdashboard');
    await expect(page.getByRole('link', { name: 'English', exact: true })).toHaveAttribute('href', '/sign-in?next=%2Fdashboard');
  }
});
