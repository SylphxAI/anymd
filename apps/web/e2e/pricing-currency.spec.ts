import { expect, test } from '@playwright/test';

const canonical = 'https://anymd.sylphx.com/pricing';
const cases = [
  ['', 'USD', '$0', '$29', 29],
  ['?currency=EUR', 'EUR', '€0', '€26.99', 26.99],
  ['?currency=HKD', 'HKD', 'HK$0', 'HK$229', 229],
  ['?currency=JPY', 'JPY', '¥0', '¥4,639', 4639],
  ['?currency=QQQ', 'USD', '$0', '$29', 29],
  ['?currency=EUR&currency=JPY', 'USD', '$0', '$29', 29],
] as const;

test.describe('server-rendered currency', () => {
  test.use({ javaScriptEnabled: false, locale: 'fr-FR' });
  test('offers, select, FAQ and a single canonical match the requested currency without JavaScript', async ({ page }) => {
    for (const [search, code, free, pro, amount] of cases) {
      const res = await page.goto('/pricing' + search);
      expect(res?.ok(), search).toBe(true);
      const html = await res!.text();
      expect(html.match(/<head(?:\s[^>]*)?>/gi)).toHaveLength(1);
      expect(html.match(/rel="canonical"/g)).toHaveLength(1);
      await expect(page.locator('head link[rel="canonical"]')).toHaveAttribute('href', canonical);
      await expect(page.getByLabel('Billing currency')).toHaveValue(code);
      await expect(page.locator('.price')).toHaveText(['' + free, pro]);
      const graph = JSON.parse(await page.locator('script[type="application/ld+json"]').textContent() ?? '')['@graph'];
      expect(graph[0].offers).toMatchObject([{ price: 0, priceCurrency: code }, { price: amount, priceCurrency: code }]);
      await expect(page.locator('.faq-list dd').first()).toHaveText(graph[1].mainEntity[0].acceptedAnswer.text);
      expect(graph[1].mainEntity[0].acceptedAnswer.text).toContain(pro);
    }
    const account = await page.goto('/account');
    expect((await account?.text())?.match(/<head(?:\s[^>]*)?>/gi)).toHaveLength(1);
    await expect(page.locator('head meta[name="robots"]')).toHaveAttribute('content', 'noindex');
  });
});

test.describe('localized checkout', () => {
  test.use({ locale: 'fr-FR' });
  test('localizes after hydration and keeps the currency across selection, history, reload and sign-in', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(() => localStorage.setItem('anymd.cookie-settings.v1', 'reject'));
    await page.goto('/pricing?currency=EUR&entry_surface=cli_pdf&checkout=cancelled#g');
    const plans = page.locator('.price');
    await expect(plans).toHaveText(['0 €', '26,99 €']);
    await expect(page.locator('.faq-list dd').first()).toContainText('26,99 €');
    const currency = page.getByLabel('Billing currency');
    await currency.selectOption('JPY');
    await expect(page).toHaveURL('/pricing?currency=JPY&entry_surface=cli_pdf&checkout=cancelled#g');
    await expect(plans).toHaveText(['0 JPY', '4 639 JPY']);
    const signIn = page.getByRole('link', { name: 'Sign in to buy Pro' });
    await expect(signIn).toHaveAttribute('href', '/sign-in?next=' + encodeURIComponent('/pricing?entry_surface=cli_pdf&currency=JPY'));
    await page.goBack();
    await expect(currency).toHaveValue('EUR');
    await expect(plans).toHaveText(['0 €', '26,99 €']);
    await page.goForward();
    await expect(currency).toHaveValue('JPY');
    await page.reload();
    await expect(currency).toHaveValue('JPY');
    await expect(plans).toHaveText(['0 JPY', '4 639 JPY']);
    expect(errors).toEqual([]);
  });
});
