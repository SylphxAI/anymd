import { expect, test, type Page } from '@playwright/test';

const locales = [
  { home: '/', studio: '/studio', preview: 'Preview', drop: 'Drop a document here', examples: [
    ['PDF', 'River survey', 'Try a PDF in Studio'],
    ['Multi-page document', 'Field brief', 'Try a document in Studio'],
    ['Video frame', 'Frame citation', 'Try a video in Studio'],
  ] },
  { home: '/zh-TW', studio: '/zh-TW/studio', preview: '預覽', drop: '將文件拖曳到這裡', examples: [
    ['PDF', '河川水質調查', '在工作室試用 PDF 範例'],
    ['多頁文件', '採樣計畫', '在工作室試用文件範例'],
    ['影片影格', '影格引用', '在工作室試用影片範例'],
  ] },
];
const samples = [
  { key: 'pdf', url: '/samples/sample.pdf', file: 'sample.pdf', result: 'Quarterly Field Report', selector: 'table', count: 1 },
  { key: 'document', url: '/samples/brief.html', file: 'brief.html', result: 'Field Notes', selector: 'li', count: 2 },
  { key: 'video', url: '/demo.mp4', file: 'demo.mp4', result: '', selector: 'li', count: 5 },
];
const campaign = 'utm_source=studio-example&utm_campaign=Field+Notes';

async function expectNoOverflow(page: Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
}

for (const locale of locales) {
  test(locale.home + ' previews all formats and opens each matching Studio sample', async ({ page, hasTouch }) => {
    const errors: string[] = [];
    const signal: string[] = [];
    await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => {
      if (message.type() === 'error' || /content security policy/i.test(message.text())) errors.push(message.text());
    });
    page.on('request', request => {
      if (new URL(request.url()).pathname === '/v1/analytics/events:batch') signal.push(request.method());
    });

    const home = locale.home + '?sample=bogus&' + campaign;
    const demo = page.locator('.home-conversion-demo');
    const panel = demo.getByRole('tabpanel');
    const raw = panel.locator('pre');
    const rendered = panel.locator('.studio-preview');
    const markdown = panel.getByRole('button', { name: 'Markdown', exact: true });
    const preview = panel.getByRole('button', { name: locale.preview, exact: true });
    const geometry = () => demo.evaluate(el => {
      const panel = el.querySelector<HTMLElement>('[role="tabpanel"]:not([hidden])')!;
      return [el.clientHeight, panel.offsetHeight, panel.querySelector('.home-conversion-demo-pane')!.clientHeight];
    });
    const activate = (control: typeof markdown) => hasTouch ? control.tap() : control.click();

    await page.goto(home);
    const skip = page.locator('.skip-link');
    await expect(skip).toHaveCSS('clip-path', 'inset(50%)');
    await page.keyboard.press('Tab');
    await expect(skip).toBeFocused();
    await expect(skip).toHaveCSS('clip-path', 'none');
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(home + '#main');
    await expect(skip).toHaveCSS('clip-path', 'inset(50%)');
    await expect(markdown).toHaveAttribute('aria-pressed', 'true');
    await expect(raw).toContainText('## ' + locale.examples[0][1]);
    await page.evaluate(() => document.fonts.ready.then(() => undefined));
    const initialGeometry = await geometry();
    await activate(preview);
    for (const [i, sample] of samples.entries()) {
      const [tab, heading] = locale.examples[i];
      await activate(demo.getByRole('tab', { name: tab, exact: true }));
      await expect(preview).toHaveAttribute('aria-pressed', 'true');
      await expect(rendered.getByRole('heading', { name: heading, exact: true })).toBeVisible();
      await expect(rendered.locator(sample.selector)).toHaveCount(sample.count);
      const size = await geometry();
      expect(size).toEqual(initialGeometry);
      expect(await rendered.evaluate(el => el.scrollWidth - el.clientWidth)).toBe(0);
      await expectNoOverflow(page);
      await activate(markdown);
      await expect(raw).toContainText('## ' + heading);
      expect(await geometry()).toEqual(size);
      await activate(preview);
    }

    for (const [i, sample] of samples.entries()) {
      const [tab, , action] = locale.examples[i];
      await page.goto(home);
      await activate(demo.getByRole('tab', { name: tab, exact: true }));
      const link = panel.getByRole('link', { name: action, exact: true });
      const destination = locale.studio + '?sample=' + sample.key + '&' + campaign;
      await expect(link).toHaveAttribute('href', destination);
      const asset = page.waitForResponse(response => new URL(response.url()).pathname === sample.url);
      await activate(link);
      await expect(page).toHaveURL(destination);
      expect((await asset).ok()).toBe(true);
      const studio = page.locator('.studio');
      await expect(studio.locator('.studio-controls')).toHaveAttribute('aria-busy', 'false');
      await expect(studio.locator('.studio-label')).toContainText(sample.file);
      if (sample.key === 'video') {
        const video = studio.locator('video');
        await expect(video).toHaveAttribute('src', '/demo.mp4');
        await expect.poll(() => video.evaluate(el => el.readyState)).toBeGreaterThanOrEqual(1);
      } else await expect(studio.locator('.studio-code')).toContainText(sample.result);
      await expectNoOverflow(page);
    }

    const unexpected: string[] = [];
    page.on('request', request => {
      const path = new URL(request.url()).pathname;
      if (samples.some(sample => sample.url === path)) unexpected.push(path);
    });
    for (const query of ['sample=bogus', 'sample=pdf&sample=video']) {
      await page.goto(locale.studio + '?' + query);
      await expect(page.getByText(locale.drop, { exact: true })).toBeVisible();
    }
    expect(unexpected).toEqual([]);
    expect(signal).toEqual([]);
    expect(errors).toEqual([]);
  });
}
