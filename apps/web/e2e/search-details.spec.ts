import { expect, test } from '@playwright/test';

const origin = 'https://anymd.sylphx.com';
const comparison = '/vs/markdownify-mcp';

for (const route of ['/', '/pricing', comparison]) {
  test(`${route} exposes canonical metadata and structured data matching visible copy`, async ({ page, request }) => {
    const url = origin + route;
    const response = await request.get(route);
    expect(response.ok()).toBe(true);
    const html = await response.text();
    expect(html).toContain('"@type":"SoftwareApplication"');
    expect(html).toContain('"@type":"FAQPage"');
    await page.goto(route);
    await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', url);
    await expect(page.locator('meta[property="og:url"]')).toHaveAttribute('content', url);
    await expect(page.locator('meta[name="twitter:card"]')).toHaveAttribute('content', 'summary_large_image');
    const graph = JSON.parse(await page.locator('script[type="application/ld+json"]').textContent() ?? '')['@graph'];
    const [software, faq] = graph;
    expect(software).toMatchObject({
      '@type': 'SoftwareApplication', name: 'Anymd', alternateName: 'Anymd by Sylphx',
      publisher: { '@type': 'Organization', name: 'Sylphx Limited', url: 'https://sylphx.com' },
    });
    expect(software.description).toMatch(/PDF and Office .*CLI and MCP/);
    expect(software.offers.map((offer: {price: number}) => offer.price)).toEqual(route === comparison ? [0] : [0, 29]);
    expect(software.offers.every((offer: {'@type': string; priceCurrency: string}) => offer['@type'] === 'Offer' && offer.priceCurrency === 'USD')).toBe(true);
    expect(faq).toMatchObject({ '@type': 'FAQPage' });
    const entries = faq.mainEntity.map(({ name, acceptedAnswer }: { name: string; acceptedAnswer: { text: string } }) => [name, acceptedAnswer.text]).flat();
    await expect(page.locator('.faq-list').locator('dt, dd')).toHaveText(entries);
  });
}

for (const [route, title, description] of [
  ['/', 'Anymd by Sylphx — local PDF & Office to Markdown CLI and MCP', /^Anymd by Sylphx converts PDF, Office .* on your machine\. Free, MIT-licensed CLI and MCP server/],
  ['/zh-TW', 'Anymd by Sylphx — 在本機把 PDF、Office 轉成 Markdown 的 CLI 與 MCP', /^Anymd 由 Sylphx 開發，在你的電腦上把 PDF、Office 文件.*CLI 與 MCP 伺服器/],
] as const) {
  test(route + ' names Anymd by Sylphx and the local PDF/Office CLI and MCP in its snippet', async ({ page }) => {
    await page.goto(route);
    await expect(page).toHaveTitle(title);
    await expect(page.locator('meta[property="og:title"]')).toHaveAttribute('content', title);
    for (const selector of ['meta[name="description"]', 'meta[property="og:description"]']) {
      await expect(page.locator(selector)).toHaveAttribute('content', description);
    }
    expect((await page.locator('meta[name="description"]').getAttribute('content'))!.length).toBeLessThanOrEqual(160);
    await expect(page.locator('link[rel="alternate"][hreflang="zh-TW"]')).toHaveAttribute('href', 'https://anymd.sylphx.com/zh-TW');
  });
}

test.describe('crawler-visible pricing FAQs', () => {
  test.use({ javaScriptEnabled: false });

  const englishTitles = [
    'How much does Anymd Pro cost?',
    'How do I activate Pro?',
    'What can I do for free?',
    'What does Pro add to the CLI and MCP server?',
    'Does Anymd upload my documents? Can I use Pro offline?',
    'Does Pro include updates? Where can I get an invoice or request a refund?',
  ];
  const traditionalChineseTitles = [
    'Anymd Pro 要多少錢？',
    '如何啟用 Pro？',
    '免費版可以做什麼？',
    'CLI 和 MCP 伺服器的 Pro 版多了哪些功能？',
    'Anymd 會上傳我的文件嗎？Pro 可以離線使用嗎？',
    'Pro 買斷後還能更新嗎？要去哪裡查看發票或申請退款？',
  ];

  for (const [path, canonical, titles] of [
    ['/pricing', '/pricing', englishTitles],
    ['/zh-TW/pricing', '/zh-TW/pricing', traditionalChineseTitles],
    ['/zh-Hant/pricing', '/zh-TW/pricing', traditionalChineseTitles],
  ] as const) {
    test(path + ' serves six matching FAQ entries with no horizontal overflow', async ({ page }) => {
      expect((await page.goto(path))?.status()).toBe(200);
      const faq = page.locator('.pricing-faq');
      await expect(faq).toBeVisible();
      await expect(faq.locator('dt')).toHaveText(titles);
      const answers = faq.locator('dd');
      await expect(answers).toHaveCount(6);
      const links = answers.last().getByRole('link');
      await expect(links).toHaveCount(3);
      await expect(links.nth(0)).toHaveAttribute('href', '/billing');
      await expect(links.nth(1)).toHaveAttribute('href', 'mailto:hi@sylphx.com');
      await expect(links.nth(1)).toHaveAttribute('rel', 'external');
      await expect(links.nth(2)).toHaveAttribute('href', '/legal/pro-terms');

      const graph = JSON.parse(await page.locator('script[type="application/ld+json"]').textContent() ?? '')['@graph'];
      const answerTexts = await answers.allTextContents();
      expect(graph.filter((node: { '@type': string }) => node['@type'] === 'FAQPage')).toEqual([{
        '@type': 'FAQPage',
        '@id': origin + canonical + '#faq-title',
        mainEntity: titles.map((title, index) => ({
          '@type': 'Question', name: title, acceptedAnswer: { '@type': 'Answer', text: answerTexts[index] },
        })),
      }]);
      expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBe(0);
    });
  }

  test('pricing FAQ links retain supported campaign parameters unless GPC is on', async ({ page }) => {
    const campaign = '?utm_source=faq_fixture';
    const path = '/zh-TW/pricing' + campaign + '&unrelated=fixture';
    const links = page.locator('.pricing-faq dd').last().getByRole('link');
    await page.goto(path);
    await expect(links.nth(0)).toHaveAttribute('href', '/billing' + campaign);
    await expect(links.nth(1)).toHaveAttribute('href', 'mailto:hi@sylphx.com');
    await expect(links.nth(2)).toHaveAttribute('href', '/legal/pro-terms' + campaign);

    await page.setExtraHTTPHeaders({ 'Sec-GPC': '1' });
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-signal-gpc', '1');
    await expect(links.nth(0)).toHaveAttribute('href', '/billing');
    await expect(links.nth(1)).toHaveAttribute('href', 'mailto:hi@sylphx.com');
    await expect(links.nth(2)).toHaveAttribute('href', '/legal/pro-terms');
  });
});

for (const [route, locale] of [['/studio', 'en'], ['/zh-TW/studio', 'zh-TW']]) {
  test(`${route} serves localized free WebApplication metadata`, async ({ request }) => {
    const response = await request.get(route + '?utm_source=github&utm_medium=readme&utm_content=nav');
    expect(response.status()).toBe(200);
    const html = await response.text();
    const canonical = origin + route;
    expect(html).toContain(`rel="canonical" href="${canonical}"`);
    const script = html.match(/<script\b[^>]*\btype="application\/ld\+json"[^>]*>([^<]+)<\/script>/);
    expect(script).not.toBeNull();
    const data = JSON.parse(script?.[1] ?? 'null');
    expect(data['@context']).toBe('https://schema.org');
    expect(data['@graph']).toEqual([expect.objectContaining({
      '@type': 'WebApplication',
      url: canonical,
      inLanguage: locale,
      description: expect.stringMatching(/Markdown/),
      browserRequirements: expect.stringMatching(/WebAssembly/),
      offers: expect.objectContaining({ '@type': 'Offer', price: 0, priceCurrency: 'USD', url: canonical }),
    })]);
  });
}

test('task index exposes the three visible guide links as a canonical ItemList', async ({ page, request }) => {
  const response = await request.get('/tasks?utm_source=docs');
  expect(response.ok()).toBe(true);
  const ssr = await response.text();
  expect(ssr).toContain('"@type":"CollectionPage"');
  expect(ssr).toContain('"@type":"ItemList"');
  await page.goto('/tasks?utm_source=docs');
  const data = JSON.parse(await page.locator('script[type="application/ld+json"]').textContent() ?? '');
  const links = await page.locator('main ul h2').evaluateAll(headings => headings.map(heading => ({
    title: heading.textContent,
    path: new URL(heading.closest('a')!.href).pathname,
  })));
  expect(links).toHaveLength(3);
  expect(data['@graph']).toEqual([{
    '@type': 'CollectionPage',
    name: 'Task guides',
    url: origin + '/tasks',
    mainEntity: {
      '@type': 'ItemList',
      itemListElement: links.map(({ title, path }, index) => ({
        '@type': 'ListItem', position: index + 1, name: title, url: origin + path,
      })),
    },
  }]);
});

test('article bridge follows the article without widening the page and is absent on excluded routes', async ({ page }) => {
  const bridge = page.getByRole('region', {name:'Try a file of your own'});
  for (const path of ['/vs/docling', '/docs/cli', '/tasks/check-pdf-citation', '/use-cases/pdf-and-office-to-markdown-mcp']) {
    expect((await page.goto(path))?.ok()).toBe(true);
    await expect(page.locator('article.prose + section.panel')).toHaveAttribute('aria-labelledby', 'article-bridge-title');
    await expect(bridge.getByRole('link', {name:'Open Document Studio'})).toBeVisible();
    await expect(bridge.getByRole('link', {name:'Anymd Pro — $29 once'})).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
  }
  for (const path of ['/docs/pro-licence-testing', '/legal/terms', '/security/advisories', '/privacy']) {
    expect((await page.goto(path))?.ok()).toBe(true);
    await expect(page.locator('article.prose')).toBeVisible();
    await expect(bridge).toHaveCount(0);
  }
});

test('comparison is discoverable and links to Studio and local setup', async ({ page }) => {
  for (const path of ['/vs', '/docs']) {
    await page.goto(path);
    await expect(page.getByRole('main').locator(`a[href="${comparison}"]`)).toBeVisible();
  }
  await page.goto(comparison);
  await expect(page).toHaveTitle(/Markdownify MCP alternative/);
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Anymd vs Markdownify MCP');
  await expect(page.getByRole('table')).toBeVisible();
  const width = page.viewportSize()!.width;
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
  await expect(page.getByRole('link', { name: 'Open the PDF sample' })).toHaveAttribute('href', '/studio?sample=pdf');
  await page.getByRole('link', { name: 'Get the CLI and MCP server' }).click();
  await expect(page).toHaveURL(comparison + '#install');
  await expect(page.getByRole('contentinfo').getByRole('link', { name: 'Comparisons' })).toHaveAttribute('href', '/vs');
});

test('homepage leads with free Studio and a priced Pro alternative; sign-in is not indexable', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('link', { name: 'Try Studio Free' })).toHaveAttribute('href', '/studio');
  await expect(page.getByRole('link', { name: 'Get Anymd Pro — $29' })).toHaveAttribute('href', '/pricing?entry_surface=home_hero');
  await expect(page.getByRole('main').getByRole('link', { name: 'Sign in', exact: true })).toHaveCount(0);
  await page.goto('/sign-in');
  await expect(page.locator('meta[name="robots"]')).toHaveAttribute('content', 'noindex');
});
