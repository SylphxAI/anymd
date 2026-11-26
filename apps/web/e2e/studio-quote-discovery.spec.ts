import { expect, type Locator, test } from '@playwright/test';

const locales = [
  { path: '/studio', pricing: '/pricing', pdf: 'Sample PDF', html: 'Sample document', input: 'Quote from the document',
    samples: 'Try a quote from this sample', found: 'Found in page 1.', missing: 'No exact match within a single source unit.',
    see: 'See the sample proof', pro: 'View Anymd Pro pricing', allow: 'Analytics only' },
  { path: '/zh-TW/studio', pricing: '/zh-TW/pricing', pdf: '範例 PDF', html: '範例文件', input: '文件中的引文',
    samples: '試查範例中的引文', found: '找到引文：第 1 頁。', missing: '未在同一頁、投影片或章節內找到完全相符的文字。',
    see: '查看範例引文證據', pro: '查看 Anymd Pro 價格', allow: '僅限分析' },
];
const pdfPhrases = ['Quarterly Field Report', 'emits one clean paragraph', 'never leave this page'];
const htmlPhrases = ['Field Notes: Water Survey', 'sampled the lower river', 'the lowest turbidity'];
const campaign = 'utm_source=studio-e2e&utm_campaign=quote';
const surface = 'studio_quote_match';
type SignalEvent = { event_name: string; properties: Record<string, unknown> };

for (const locale of locales) {
  test(locale.path + ' finds real sample quotes and opens PDF proof without sending anything before choice', async ({ page, hasTouch }, info) => {
    const signalRequests: string[] = [];
    const events: SignalEvent[] = [];
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    await page.addInitScript(() => {
      const violations: string[] = [];
      Reflect.set(window, 'studioCspViolations', violations);
      document.addEventListener('securitypolicyviolation', event => violations.push(event.violatedDirective));
    });
    await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
    await page.route('https://www.googletagmanager.com/**', route => route.fulfill({ contentType: 'application/javascript', body: '' }));
    await page.route('**/v1/analytics/events:batch', route => {
      const request = route.request();
      signalRequests.push(request.method());
      const headers = { 'Access-Control-Allow-Origin': '*', 'Access-Control-Allow-Methods': 'POST',
        'Access-Control-Allow-Headers': 'content-type, x-sylphx-key' };
      if (request.method() === 'OPTIONS') return route.fulfill({ status: 204, headers });
      events.push(...request.postDataJSON().events as SignalEvent[]);
      // Assert the real request shape without sending test traffic to production Signal.
      return route.fulfill({ status: 200, headers, json: {} });
    });
    const response = await page.goto(locale.path + '?' + campaign + '&private=never-forward');
    expect(response?.ok()).toBe(true);
    expect(response?.headers()['content-security-policy']).toMatch(/script-src [^;]*'nonce-/);
    const allow = page.getByRole('button', { name: locale.allow, exact: true });
    await expect(allow).toBeEnabled();

    const activate = async (control: Locator) => {
      await expect(control).toBeEnabled();
      await (hasTouch ? control.tap() : control.press('Enter'));
    };
    const samples = page.getByRole('group', { name: locale.samples, exact: true });
    const input = page.getByRole('textbox', { name: locale.input });
    const mark = page.locator('.studio-excerpts mark');
    const bridge = page.locator('.studio-quote-proof');
    const pro = bridge.getByRole('link', { name: locale.pro });
    const file = page.locator('.studio input[type=file]');
    const pricing = locale.pricing + '?entry_surface=' + surface;
    const checkWidth = async () => {
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
      expect(await page.locator('.studio-quote').evaluate(el => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(0);
      await expect(page.locator('.studio-quote [style]:not([style=""])')).toHaveCount(0);
    };
    const tryPhrases = async (phrases: string[]) => {
      await expect(samples.getByRole('button')).toHaveCount(phrases.length);
      // End with the first phrase (the same text as the PDF's recorded crop).
      for (const phrase of [...phrases.slice(1), phrases[0]]) {
        await activate(samples.getByRole('button', { name: phrase, exact: true }));
        await expect(input).toHaveValue(phrase);
        await expect(mark).toHaveText(phrase);
      }
      await checkWidth();
    };

    await activate(page.getByRole('button', { name: locale.html, exact: true }));
    await tryPhrases(htmlPhrases);
    await expect(bridge).toBeHidden();

    const pdfRequest = page.waitForResponse('**/samples/sample.pdf');
    await activate(page.getByRole('button', { name: locale.pdf, exact: true }));
    const pdf = await pdfRequest;
    expect(pdf.ok()).toBe(true);
    const pdfBytes = await pdf.body();
    await expect(input).toHaveValue('');
    await expect(bridge).toBeHidden();
    await tryPhrases(pdfPhrases);
    await expect(page.getByRole('status').filter({ hasText: locale.found })).toBeVisible();
    await expect(bridge).toContainText('cite_check');
    await expect(pro).toHaveAttribute('href', pricing + '&' + campaign);
    await page.locator('.studio-quote').screenshot({ caret: 'initial', path: info.outputPath('quote-search.png') });

    const details = page.locator('#studio-cite-proof');
    const crop = details.getByRole('img');
    await expect(crop).toBeHidden();
    await activate(bridge.getByRole('link', { name: locale.see }));
    await expect(page).toHaveURL(/#studio-cite-heading$/);
    await expect(page.locator('#studio-cite-heading')).toBeFocused();
    await expect(details).toHaveJSProperty('open', true);
    await expect(crop).toBeInViewport();
    await expect(crop.locator('rect')).toBeVisible();
    await checkWidth();
    await page.locator('.cite-preview').screenshot({ path: info.outputPath('citation-proof.png') });

    // Same bytes and same name, but input[type=file] is not the built-in sample.
    await file.setInputFiles({ name: 'sample.pdf', mimeType: 'application/pdf', buffer: pdfBytes });
    await expect(input).toHaveValue('');
    await expect(samples).toHaveCount(0);
    await expect(bridge).toBeHidden();
    await input.fill('not an actual quote');
    await input.press('Enter');
    await expect(page.getByRole('status').filter({ hasText: locale.missing })).toBeVisible();
    await expect(bridge).toBeHidden();
    await input.fill(pdfPhrases[0]);
    await input.press('Enter');
    await expect(mark).toHaveText(pdfPhrases[0]);
    await expect(pro).toHaveAttribute('href', pricing + '&' + campaign);
    await activate(pro);
    await expect(page).toHaveURL(pricing + '&' + campaign);
    expect(signalRequests).toEqual([]);
    expect(await page.evaluate(() => Reflect.get(window, 'studioCspViolations'))).toEqual([]);

    // Before consent there is a real link, but zero behavioral Signal requests.
    // After consent exactly one CTA event is projected and the URL label survives.
    await page.goto(locale.path + '?' + campaign + '&private=never-forward');
    await activate(page.getByRole('button', { name: locale.pdf, exact: true }));
    await activate(samples.getByRole('button', { name: pdfPhrases[0], exact: true }));
    await expect(mark).toHaveText(pdfPhrases[0]);
    expect(signalRequests).toEqual([]);
    await allow.click();
    await expect(pro).toHaveAttribute('href', pricing);
    await activate(pro);
    await expect(page).toHaveURL(pricing);
    const ctaEvents = () => events.filter(event => event.event_name === 'pro_cta_clicked').map(event => event.properties);
    await expect.poll(ctaEvents).toEqual([{ entry_surface: surface, path: locale.path, utm_source: 'studio-e2e', utm_campaign: 'quote' }]);
    expect(await page.evaluate(() => Reflect.get(window, 'studioCspViolations'))).toEqual([]);
    expect(errors).toEqual([]);
  });
}
