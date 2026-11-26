import { expect, test } from '@playwright/test';
import { signalChoiceKey } from '../src/lib/consent';

const articlePath = '/use-cases/pdf-and-office-to-markdown-mcp';
const surface = 'entry_surface=seo_pdf-and-office-to-markdown-mcp';
const samplePath = '/studio?sample=pdf&' + surface;
const campaign = 'gclid=FIXTURE_PDF&utm_source=editorial-fixture&utm_medium=cpc&utm_campaign=PDF+sample';
const incoming = campaign + '&entry_surface=cli_pdf&sample=video&sample=document&unrelated=drop-me';

for (const mode of ['ordinary', 'before consent', 'GPC'] as const) {
  test('PDF and Office article opens the local PDF sample: ' + mode, async ({ page, context }) => {
    const tracking: string[] = [];
    await context.route('**/cdn-cgi/trace', route => route.fulfill({body:'loc=DE\n'}));
    // Fail on attempted telemetry or uploads without letting fixture traffic leave the browser.
    await context.route('**/*', route => {
      const request = route.request();
      const url = new URL(request.url());
      if (!['GET', 'HEAD'].includes(request.method()) || url.pathname === '/v1/analytics/events:batch' ||
        /(?:^|\.)(?:googletagmanager|google-analytics|googleadservices|google|doubleclick)\.(?:com|net)$/.test(url.hostname)) {
        tracking.push(request.method() + ' ' + url);
        return route.abort();
      }
      return route.fallback();
    });
    if (mode === 'GPC') {
      await page.setExtraHTTPHeaders({'Sec-GPC':'1'});
      await page.addInitScript(key => {
        localStorage.setItem(key, 'granted');
        Object.defineProperty(navigator, 'globalPrivacyControl', {value:true});
      }, signalChoiceKey);
    }
    const response = await page.goto(articlePath + (mode === 'ordinary' ? '' : '?' + incoming));
    expect(response?.status()).toBe(200);
    if (mode !== 'GPC') await expect(page.getByRole('button', {name:'Reject optional', exact:true})).toBeEnabled();
    await expect(page.locator('html')).toHaveAttribute('lang', 'en');

    const article = page.locator('article.prose[data-interactive=true]');
    const sample = article.getByRole('link', {name:'Try the free PDF sample', exact:true});
    const install = article.getByRole('link', {name:'Install the free CLI and MCP server', exact:true});
    const pro = article.getByRole('link', {name:'See Anymd Pro — US$29 one-time', exact:true});
    const suffix = mode === 'before consent' ? '&' + campaign : '';
    for (const [link, href] of [[sample, samplePath], [install, '/docs?' + surface], [pro, '/pricing?' + surface]] as const) {
      await expect(link).toHaveAttribute('href', href + suffix);
      await expect(link).toBeInViewport({ratio:1});
      expect(await link.evaluate(element => {
        const box = element.getBoundingClientRect();
        return element.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2));
      })).toBe(true);
    }
    const navigation = page.getByRole('navigation', {name:'Try a sample'});
    for (const name of ['Sample PDF', 'PDF citation proof', 'HTML brief']) {
      await expect(navigation.getByRole('link', {name, exact:true})).toHaveCount(1);
    }

    for (let i = 0; i < 30 && !await sample.evaluate(link => link === document.activeElement); ++i) {
      await page.keyboard.press('Tab');
    }
    await expect(sample).toBeFocused();
    if (mode !== 'GPC') expect(await page.evaluate(key => localStorage.getItem(key), signalChoiceKey)).toBeNull();
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(samplePath + suffix);
    await expect(page.locator('html')).toHaveAttribute('lang', 'en');
    const output = page.getByLabel('Markdown output', {exact:true});
    await expect(output).toContainText('source: sample.pdf', {timeout:15_000});
    await expect(output).toContainText('# Quarterly Field Report');
    await expect(output).toContainText('<!-- page 2 -->');
    expect(await page.evaluate(() => Reflect.get(window, 'dataLayer') ?? [])).toEqual([]);
    expect(tracking).toEqual([]);
  });
}
