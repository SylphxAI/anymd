import { expect, test, type Page } from '@playwright/test';

async function heroGeometry(page: Page) {
  await page.evaluate(async () => {
    await document.fonts.ready;
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
  return page.evaluate(() => {
    const box = (selector: string) => {
      const { x, y, width, height } = document.querySelector(selector)!.getBoundingClientRect();
      return [x, y, width, height];
    };
    const tabs = document.querySelector('.home-conversion-demo-tabs')!;
    return {
      grid: getComputedStyle(tabs).display,
      boxes: [box('.hero-preview'), box('.home-conversion-demo-panel:not([hidden]) .home-conversion-demo-link'), box('.format-strip')],
      overflow: document.documentElement.scrollWidth - innerWidth,
    };
  });
}

for (const path of ['/', '/zh-TW']) {
  test(`${path} paints the complete home demo before hydration`, async ({ page }) => {
    // Hold the async client entry so a fast cache or runner cannot hide an unstyled SSR frame.
    let hydrate!: () => void;
    const client = new Promise<void>(resolve => { hydrate = resolve; });
    const entry = '**/_build/assets/entry-client-*.js';
    await page.route(entry, async route => {
      await client;
      await route.continue();
    });
    await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
    try {
      const [response] = await Promise.all([
        page.goto(path, { waitUntil: 'domcontentloaded' }),
        page.waitForRequest(entry),
      ]);
      expect(response?.ok()).toBe(true);
      const first = await heroGeometry(page);
      expect(first.grid).toBe('grid');
      expect(first.boxes[1][3]).toBeGreaterThanOrEqual(44);
      expect(first.overflow).toBeLessThanOrEqual(0);
      hydrate();
      await expect(page.locator('.cookie-panel')).toBeVisible();
      const hydrated = await heroGeometry(page);
      expect(hydrated.overflow).toBeLessThanOrEqual(0);
      for (const [index, finalBox] of hydrated.boxes.entries()) {
        for (const [dimension, value] of finalBox.entries()) {
          expect(Math.abs(value - first.boxes[index][dimension])).toBeLessThanOrEqual(1);
        }
      }
    } finally {
      hydrate();
    }
  });
}
