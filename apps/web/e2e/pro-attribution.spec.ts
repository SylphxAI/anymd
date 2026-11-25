import { expect, type Page, test } from '@playwright/test';
import { signalChoiceKey } from '../src/lib/consent';

const docs = [
  ['/vs/docling', 'seo_vs-docling', '/vs', 'See Anymd Pro'],
  ['/tasks/check-pdf-citation', 'task_check_pdf_citation', '/tasks', 'See Anymd Pro — US$29 one-time'],
] as const;

type Event = { event_name: string; properties: Record<string, unknown> };
async function observe(page: Page, region: 'DE' | 'US') {
  const requests: string[] = [];
  const ctaEvents: Event[] = [];
  await page.route('**/cdn-cgi/trace', route => route.fulfill({body:'loc=' + region + '\n'}));
  await page.route(url => /(?:^|\.)(?:googletagmanager|google-analytics|googleadservices|google|doubleclick)\.(?:com|net)$/.test(url.hostname),
    route => route.fulfill({contentType:'application/javascript', body:''}));
  await page.route(url => url.pathname === '/v1/analytics/events:batch', route => {
    const request = route.request();
    requests.push(request.method());
    const headers = {'Access-Control-Allow-Origin':'*', 'Access-Control-Allow-Methods':'POST', 'Access-Control-Allow-Headers':'content-type, x-sylphx-key'};
    if (request.method() === 'OPTIONS') return route.fulfill({status:204, headers});
    const {events} = request.postDataJSON() as {events: Event[]};
    ctaEvents.push(...events.filter(event => event.event_name === 'pro_cta_clicked'));
    return route.fulfill({json:{}, headers});
  });
  return {requests, ctaEvents};
}

const gaCtaSurfaces = (page: Page) => page.evaluate(() => {
  const rows = (Reflect.get(window, 'dataLayer') ?? []) as [string, string, {entry_surface?: string}][];
  return rows.filter(row => row[0] === 'event' && row[1] === 'pro_cta_clicked').map(row => row[2].entry_surface);
});

for (const mode of ['pre-consent', 'Sec-GPC', 'analytics only'] as const) {
  test('organic Bridge and markdown CTA attribution with ' + mode, async ({page}) => {
    test.skip(mode === 'analytics only' && Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Opt-in click verification uses the loopback binding');
    const {ctaEvents, requests} = await observe(page, mode === 'analytics only' ? 'DE' : 'US');
    if (mode === 'Sec-GPC') {
      await page.setExtraHTTPHeaders({'Sec-GPC':'1'});
      await page.addInitScript(key => localStorage.setItem(key, 'granted'), signalChoiceKey);
    }
    const expected: Record<string, string>[] = [];
    for (const [path, surface, section, inlineLabel] of docs) {
      const response = await page.goto(path + '?proof=private-input');
      expect(response?.status()).toBe(200);
      if (mode === 'Sec-GPC') expect(response?.headers().vary?.toLowerCase()).toContain('sec-gpc');
      const article = page.locator('article.prose');
      await expect(article).toHaveAttribute('data-interactive', 'true');
      if (mode === 'analytics only') {
        const optIn = page.getByRole('button', {name:'Analytics only', exact:true});
        if (await optIn.isVisible()) await optIn.click();
        await expect(page.locator('#anymd-google-tag')).toHaveCount(1);
      }
      const scans = [
        article.getByRole('link', {name:inlineLabel, exact:true}),
        page.getByRole('region', {name:'Try a file of your own'}).getByRole('link', {name:/Anymd Pro/}),
      ];
      const href = '/pricing?entry_surface=' + surface;
      for (const [index, cta] of scans.entries()) {
        await expect(cta).toHaveAttribute('href', href);
        if (index === 0) await expect(cta).toHaveAttribute('data-doc-href', href);
        await cta.click();
        await expect(page).toHaveURL(href);
        if (mode === 'analytics only') {
          expected.push({entry_surface:surface, path:section});
          await expect.poll(() => ctaEvents.map(event => event.properties)).toEqual(expected);
        } else {
          expect(await gaCtaSurfaces(page)).toEqual([]);
          expect(requests).toEqual([]);
        }
        if (index === 0) {
          await page.goBack();
          await expect(article).toHaveAttribute('data-interactive', 'true');
        }
      }
    }
    await page.goto('/docs');
    await expect(page.getByRole('region', {name:'Try a file of your own'}).getByRole('link', {name:/Anymd Pro/}))
      .toHaveAttribute('href', '/pricing?entry_surface=article_bridge');
    if (mode !== 'analytics only') expect(requests).toEqual([]);
  });
}

test('pricing language switch keeps both allowlisted values and strips unknown or repeated values', async ({page}) => {
  const {requests} = await observe(page, 'US');
  for (const [incoming, retained] of [
    ['?entry_surface=seo_vs-docling&currency=HKD', '?entry_surface=seo_vs-docling&currency=HKD'],
    ['?entry_surface=injected_bad', ''],
    ['?entry_surface=home_hero&entry_surface=task_check_pdf_citation&currency=HKD&currency=USD', ''],
  ]) {
    await page.goto('/pricing' + incoming);
    const zh = page.getByRole('link', {name:'繁體中文', exact:true});
    await expect(zh).toHaveAttribute('href', '/zh-TW/pricing' + retained);
    await zh.click();
    await expect(page).toHaveURL('/zh-TW/pricing' + retained);
    const en = page.getByRole('link', {name:'English', exact:true});
    await expect(en).toHaveAttribute('href', '/pricing' + retained);
    await en.click();
    await expect(page).toHaveURL('/pricing' + retained);
  }
  expect(requests).toEqual([]);
});
