import { expect, type Page } from '@playwright/test';
import { test } from './fixture-test';

const copy = {
  en: { pricing: '/pricing', signIn: '/sign-in', cta: 'Sign in to buy Pro', label: 'YOUR PURCHASE', back: 'Back to pricing', google: 'Continue with Google', checkout: 'Continue to secure checkout' },
  'zh-TW': { pricing: '/zh-TW/pricing', signIn: '/zh-TW/sign-in', cta: '登入以購買 Pro', label: '你的購買', back: '返回價格頁面', google: '使用 Google 登入', checkout: '前往安全結帳' },
} as const;
const summary = (page: Page) => page.getByRole('region', { name: /^Anymd Pro · / });

function countCheckouts(page: Page) {
  const calls: unknown[] = [];
  page.on('request', request => { if (new URL(request.url()).pathname === '/api/anymd/checkout') calls.push(request.postDataJSON()); });
  return calls;
}

for (const locale of ['en', 'zh-TW'] as const) {
  test(locale + ' keeps the selected Pro purchase visible through sign-in and resumes at one checkout action', async ({ page, request }) => {
    const t = copy[locale];
    const checkouts = countCheckouts(page);
    const query = '?entry_surface=home_hero&currency=HKD';
    const next = t.pricing + query;
    await page.goto(next);
    await page.getByRole('link', { name: t.cta, exact: true }).click();
    await expect(page).toHaveURL(t.signIn + '?next=' + encodeURIComponent(next));
    await expect(summary(page)).toHaveAccessibleName('Anymd Pro · HK$229');
    await expect(summary(page)).toContainText(t.label);
    await expect(summary(page).locator('code')).toHaveText('anymd login');
    await expect(summary(page).getByRole('link', { name: t.back })).toHaveAttribute('href', next);
    // The summary ships in the server HTML, so it cannot move the sign-in form on hydration.
    const html = await (await request.get(t.signIn + '?next=' + encodeURIComponent(next))).text();
    expect(html).toMatch(/Anymd Pro · (?:<!--\$-->)?HK\$229/);

    await page.getByRole('link', { name: t.google, exact: true }).click();
    await expect(page).toHaveURL(next);
    await expect(page.getByLabel(locale === 'en' ? 'Billing currency' : '結帳幣別')).toHaveValue('HKD');
    // Returning signed in never starts checkout or grants delivery consent on the buyer's behalf.
    await expect(page.getByRole('checkbox')).not.toBeChecked();
    const buy = page.getByRole('button', { name: t.checkout });
    await expect(buy).toBeDisabled();
    await page.reload();
    await expect(buy).toBeDisabled();
    expect(checkouts).toEqual([]);

    let release = () => {};
    const held = new Promise<void>(resolve => { release = resolve; });
    await page.route('**/api/anymd/checkout', async route => {
      await held;
      await route.fulfill({ status: 503, json: { code: 'checkout_unconfigured' } });
    });
    await page.getByRole('checkbox').check();
    await buy.dblclick();
    await expect(page.getByRole('button', { name: locale === 'en' ? 'Opening checkout…' : '正在開啟結帳…' })).toBeDisabled();
    release();
    await expect(page.getByRole('alert')).toBeVisible();
    await expect(page).toHaveURL(next);
    expect(checkouts).toHaveLength(1);
    expect(checkouts[0]).toMatchObject({ currency: 'HKD', consent: true, entrySurface: 'home_hero' });
  });
}

test('back from sign-in returns to the same harmless pricing selection', async ({ page }) => {
  await page.goto('/sign-in?error=cancelled&next=' + encodeURIComponent('/pricing?currency=EUR'));
  await expect(page.getByRole('alert')).toBeVisible();
  await expect(summary(page)).toHaveAccessibleName(/^Anymd Pro · €26\.99$/);
  await summary(page).getByRole('link', { name: 'Back to pricing' }).click();
  await expect(page).toHaveURL('/pricing?currency=EUR');
  await expect(page.getByLabel('Billing currency')).toHaveValue('EUR');
});

for (const next of ['//example.invalid/pricing', '/pricing?currency=ZZZ', '/pricing?amount=1&currency=USD', '/pricing?currency=USD&currency=HKD', '/dashboard', '/recover?session_id=cs_fixture']) {
  test('shows no purchase for return target ' + next, async ({ page }) => {
    await page.goto('/sign-in?next=' + encodeURIComponent(next));
    await expect(page.getByRole('link', { name: 'Continue with Google' })).toBeVisible();
    await expect(summary(page)).toHaveCount(0);
    await expect(page.getByText('YOUR PURCHASE')).toHaveCount(0);
  });
}
