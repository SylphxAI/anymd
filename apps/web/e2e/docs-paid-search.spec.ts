import { expect, test } from '@playwright/test';

const landings = [
  { gclid: 'TEST123', path: '/tasks/check-pdf-citation', surface: 'task_check_pdf_citation', proof: /page 1 → verified_exact in the page\s+box, unmatched in the empty box/ },
  { gclid: 'TEST456', path: '/tasks/extract-video-frame', surface: 'task_extract_video_frame', proof: /1031 ms → decoded frame at 1100 ms,\s+with the PNG hash/ },
  { gclid: 'TEST789', path: '/use-cases/pdf-and-office-to-markdown-mcp', surface: 'seo_pdf-and-office-to-markdown-mcp', proof: /96\.4 overall score across 38 documents converted in 11\.5 seconds/ },
];
const ctaLabel = 'See Anymd Pro — US$29 one-time';

for (const { gclid, path, surface, proof } of landings) {
  test(path + ' shows proof and carries the campaign to pricing before consent', async ({ page }) => {
    const campaign = { gclid, gbraid:'Good_A', wbraid:'Good_B', utm_source:'google', utm_medium:'cpc', utm_campaign:'PDF & video' };
    const incoming = new URLSearchParams({ ...campaign, entry_surface:'cli_pdf', email:'do-not-forward@example.invalid', utm_content:'bad\u0000value' });
    incoming.append('utm_term', 'duplicate-one');
    incoming.append('utm_term', 'duplicate-two');
    await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
    const googleRequests: string[] = [];
    await page.route(url => /(?:^|\.)(?:google\.com|googletagmanager\.com|google-analytics\.com|googleadservices\.com|doubleclick\.net)$/.test(url.hostname), route => {
      googleRequests.push(route.request().url());
      return route.abort();
    });
    const response = await page.goto(path + '?' + incoming);
    expect(response?.status()).toBe(200);
    const href = '/pricing?' + new URLSearchParams({ entry_surface: surface, ...campaign });
    // Read the markup itself, not the async payload or a hydrated replacement.
    const html = (await response!.text()).match(/<article\b[^>]*\bclass="prose"[^>]*>[\s\S]*?<\/article>/)?.[0];
    expect(html).toContain('href="' + href.replaceAll('&', '&amp;') + '"');
    expect(html).not.toMatch(/<script|do-not-forward|duplicate-one|bad%00value|8\.5\.5|utm_source=docs_/);

    const banner = page.getByRole('complementary', { name: 'Cookie settings' });
    await expect(banner.getByRole('button', { name: 'Allow analytics & ads' })).toBeEnabled();
    const article = page.locator('article.prose');
    const cta = article.getByRole('link', { name: ctaLabel, exact: true });
    const sample = article.locator('blockquote').filter({ hasText: proof });
    await expect(cta).toHaveClass(/button-primary/);
    await expect(cta).toHaveAttribute('href', href);
    const nonPageLinks = await article.locator('a[href^="/examples/"], a[href^="https://"], a[href^="//"]').evaluateAll(links => links.map(link => (link as HTMLAnchorElement).href));
    expect(nonPageLinks.length).toBeGreaterThan(0);
    for (const link of nonPageLinks) {
      const parameters = new URL(link).searchParams;
      for (const key of Object.keys(campaign)) expect(parameters.has(key)).toBe(false);
    }
    expect(await page.evaluate(() => scrollY)).toBe(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
    const dock = (await banner.boundingBox())!;
    for (const target of [sample, cta]) {
      await expect(target).toBeInViewport({ ratio: 1 });
      const box = (await target.boundingBox())!;
      expect(box.y + box.height).toBeLessThan(dock.y);
    }
    await cta.click();
    await expect(page).toHaveURL(href);
    await expect(banner.getByRole('button', { name: 'Allow analytics & ads' })).toBeEnabled();
    const signIn = page.getByRole('link', { name: 'Sign in to buy Pro', exact: true });
    const next = '/pricing?' + new URLSearchParams({ entry_surface: surface });
    await expect(signIn).toHaveAttribute('href', '/sign-in?' + new URLSearchParams({ next, ...campaign }));
    expect(await page.evaluate(() => Object.keys(localStorage).filter(key => key.startsWith('anymd.signal.') || key === 'anymd.cookie-settings.v1'))).toEqual([]);
    expect((await page.context().cookies()).filter(cookie => /^(?:_ga|_gcl_|anymd\.cookie|anymd_attribution)/.test(cookie.name))).toEqual([]);
    expect(googleRequests).toEqual([]);
  });
}
