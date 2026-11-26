import { expect, type Page, test } from '@playwright/test';

const locales = [
  { path:'/studio', pdf:'Sample PDF', drop:'Drop a document here', output:'Markdown output',
    preview:'Preview', markers:['page 1', 'page 2'], copy:'Copy Markdown', copied:'Copied',
    quote:'Quote from the document', found:'Found in page 1.' },
  { path:'/zh-TW/studio', pdf:'範例 PDF', drop:'將文件拖曳到這裡', output:'Markdown 輸出',
    preview:'預覽', markers:['第 1 頁', '第 2 頁'], copy:'複製 Markdown', copied:'已複製',
    quote:'文件中的引文', found:'找到引文：第 1 頁。' },
];

type Shift = PerformanceEntry & { value: number; hadRecentInput: boolean };
declare global { interface Window { studioLayoutShifts: () => Shift[] } }

async function cls(page: Page) {
  return page.evaluate(async () => {
    await document.fonts.ready;
    // Let the readiness DOM update paint before consuming the observer queue.
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    let max = 0, sum = 0, first = -Infinity, last = -Infinity;
    for (const { value, startTime } of window.studioLayoutShifts()) {
      if (startTime - last >= 1000 || startTime - first >= 5000) {
        first = startTime;
        sum = 0;
      }
      last = startTime;
      sum += value;
      max = Math.max(max, sum);
    }
    return max;
  });
}

// Measure the actual CSS viewport, including narrow windows without device emulation.
// Other Studio journeys exercise touch in the mobile projects.
test.use({ isMobile: false });

for (const locale of locales) {
  test(locale.path + ' keeps the PDF deep link stable on cold and warm navigation', async ({ page, context }, info) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await page.addInitScript(() => {
      const shifts: Shift[] = [];
      const collect = (entries: PerformanceEntry[]) => shifts.push(...(entries as Shift[]).filter(entry => !entry.hadRecentInput));
      const observer = new PerformanceObserver(list => collect(list.getEntries()));
      observer.observe({ type:'layout-shift', buffered:true });
      window.studioLayoutShifts = () => {
        collect(observer.takeRecords());
        return shifts;
      };
    });

    const studio = page.locator('.studio');
    const output = studio.getByLabel(locale.output, { exact:true });
    // No routing/mocking: Playwright routing would disable the warm HTTP cache.
    for (const state of ['cold', 'warm']) {
      await page.goto(locale.path + '?sample=pdf');
      await expect(output).toContainText('Quarterly Field Report', { timeout:15_000 });
      const score = await cls(page);
      await info.attach(state + ' CLS', { body:String(score), contentType:'text/plain' });
      expect.soft(score, state + ' no-input session-window CLS').toBeLessThanOrEqual(0.10);
    }

    // Only interact after both no-input measurements.
    const markdown = await output.textContent();
    const copy = studio.getByRole('button', { name:locale.copy, exact:true });
    await copy.press('Enter');
    await expect(studio.getByRole('button', { name:locale.copied, exact:true })).toBeVisible();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(markdown);

    const preview = studio.getByRole('button', { name:locale.preview, exact:true });
    await preview.press('Enter');
    await expect(preview).toBeFocused();
    await expect(studio.locator('.studio-marker')).toHaveText(locale.markers);
    await expect(studio.getByRole('table').locator('tbody tr').first()).toBeVisible();
    const quote = studio.getByLabel(locale.quote, { exact:true });
    await quote.fill('Quarterly Field Report');
    await quote.press('Enter');
    await expect(studio.getByRole('status').filter({ hasText:locale.found })).toBeVisible();

    await page.goto(locale.path);
    await expect(studio.getByText(locale.drop, { exact:true })).toBeVisible();
    await expect(studio.locator('.studio-output-pane')).toHaveCount(0);
    await expect(studio.locator('.studio-output-actions, .studio-quote')).toHaveCount(0);
    expect(await cls(page)).toBeLessThanOrEqual(0.10);
    await studio.getByRole('button', { name:locale.pdf, exact:true }).press('Enter');
    await expect(output).toContainText('Quarterly Field Report', { timeout:15_000 });
  });
}
