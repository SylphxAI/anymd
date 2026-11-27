import { createHash } from 'node:crypto';
import { expect, test } from '@playwright/test';
import { fixtureAuth } from './origins';

async function frenchRegion(page: import('@playwright/test').Page) {
  await page.route('**/cdn-cgi/trace', route => route.fulfill({body:'loc=FR\n'}));
}

test('enforces a per-response CSP without blocking the public purchase journey', async ({ page }) => {
  await frenchRegion(page);
  await page.addInitScript(() => {
    const blocked: string[] = [];
    window.addEventListener('securitypolicyviolation', e => blocked.push(e.violatedDirective + ': ' + e.blockedURI));
    Reflect.set(window, '__blocked', blocked);
  });
  const response = (await page.goto('/'))!;
  const policy = (await response.headerValue('content-security-policy'))!;
  const nonce = /'nonce-([^']+)'/.exec(policy)?.[1];
  expect(nonce).toBeTruthy();
  expect(policy).toContain("default-src 'none'");
  expect(policy).toContain("frame-ancestors 'none'");
  expect(policy.match(/script-src [^;]+/)?.[0]).not.toMatch(/'unsafe-inline'|'unsafe-eval'/);
  const connect = policy.match(/connect-src ([^;]+)/)?.[1].split(' ');
  expect(connect).toEqual(expect.arrayContaining([
    "'self'", 'https://api.sylphx.com', 'https://www.google.com', 'https://www.googleadservices.com',
    'https://googleads.g.doubleclick.net', 'https://stats.g.doubleclick.net',
    ...(!process.env.PLAYWRIGHT_BASE_URL ? [fixtureAuth] : []),
  ]));
  expect(connect).not.toContain('https:');
  expect(connect).not.toContain('*');
  expect(await page.locator('script[nonce]').first().evaluate((s: HTMLScriptElement) => s.nonce)).toBe(nonce);
  await expect(page.getByRole('button', {name:'Reject optional'})).toBeEnabled();
  const desktop = page.viewportSize()!.width > 690;
  if (!desktop) await page.locator('summary[aria-label="Open navigation"]').click();
  await page.getByRole('navigation', {name:desktop ? 'Main' : 'Mobile', exact:true}).getByRole('link', {name:'Pricing'}).click();
  await page.getByRole('link', {name:'Sign in to buy Pro'}).click();
  await expect(page).toHaveURL(url => url.pathname === '/sign-in');
  await expect(page.getByRole('heading', {name:'Sign in to Anymd', exact:true})).toBeVisible();
  expect(await page.evaluate(() => Reflect.get(window, '__blocked'))).toEqual([]);
  const fresh = (await page.request.get('/pricing')).headers()['content-security-policy'];
  expect(fresh).toContain("script-src ");
  expect(fresh).not.toContain("'nonce-" + nonce + "'");
});

test('Pricing puts the Pro price and action above the compact cookie dock', async ({ page }) => {
  await frenchRegion(page);
  const cropRequests: string[] = [];
  page.on('request', request => {
    if (/\/sample-cite-crop-[^/]+\.png$/.test(new URL(request.url()).pathname)) cropRequests.push(request.url());
  });
  await page.goto('/pricing', {waitUntil:'load'});
  const dock = page.getByRole('complementary', {name:'Cookie settings'});
  await expect(dock).toBeVisible();
  const plans = page.locator('.pricing-page .columns > article');
  await expect(plans).toHaveCount(2);
  await expect(plans.first().getByRole('heading', {level:2})).toHaveText('Anymd Pro');
  const price = plans.first().getByText('$29', {exact:true});
  const action = plans.first().getByRole('link', {name:'Sign in to buy Pro'});
  await expect(price).toBeInViewport({ratio:1});
  await expect(action).toBeInViewport({ratio:1});
  const bar = (await dock.boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(bar.height).toBeLessThanOrEqual(viewport.width === 390 ? 84 : 58);
  for (const target of [price, action]) {
    const bounds = (await target.boundingBox())!;
    expect(bounds.y + bounds.height + 8).toBeLessThanOrEqual(bar.y);
  }
  if (viewport.width === 390) {
    expect((await plans.first().boundingBox())!.y).toBeLessThan((await plans.last().boundingBox())!.y);
  }
  for (const label of ['Reject optional', 'Analytics only', 'Allow analytics & ads']) {
    const button = dock.getByRole('button', {name:label, exact:true});
    await expect(button).toBeInViewport({ratio:1});
    const {width, height} = (await button.boundingBox())!;
    expect(Math.min(width, height)).toBeGreaterThanOrEqual(44);
  }
  // The recorded PDF proof and the Free/Pro difference sit beside Buy, from one crop download.
  const buy = plans.first().locator('.pro-buy');
  await expect(buy.locator('.pro-claim')).toHaveText('Verify a PDF quote on the original page. $29 once.');
  const pdf = buy.getByRole('img', {name:'Page 1 of the sample PDF with “Quarterly Field Report” boxed', exact:true});
  await expect(pdf.locator('rect')).toBeInViewport({ratio:1});
  const crop = (await pdf.boundingBox())!;
  expect(crop.y + crop.height + 8).toBeLessThanOrEqual(bar.y);
  await expect(buy.getByText('Exact quote verified', {exact:true})).toBeInViewport({ratio:1});
  await expect(buy.getByRole('link', {name:'sample.pdf'})).toHaveAttribute('href', '/samples/sample.pdf#page=1');
  const actionBox = (await action.boundingBox())!;
  if (viewport.width === 390) expect(crop.y + crop.height).toBeLessThanOrEqual(actionBox.y);
  else {
    expect(crop.x + crop.width).toBeLessThanOrEqual(actionBox.x);
    expect(crop.y).toBeLessThan(actionBox.y + actionBox.height);
  }
  await expect(buy.locator('.plan-diff dt')).toHaveText(['Free OSS', 'Pro']);
  await expect(buy.locator('.plan-diff dd')).toHaveText(['Converts PDF and Office files to Markdown.', 'Checks quotes in your own PDFs locally, in the CLI or MCP server.']);
  const sample = await page.request.get('/samples/sample.pdf');
  await buy.getByText('PDF SHA-256', {exact:true}).click();
  await expect(buy.locator('details code')).toHaveText(createHash('sha256').update(await sample.body()).digest('hex'));
  expect(cropRequests).toHaveLength(1);
  await expect(page.locator('.pricing-page [role=img][aria-label*="Quarterly Field Report"]')).toHaveCount(1);
  const proof = plans.first().getByRole('region', {name:'Video frames too'});
  const frame = proof.getByRole('img', {name:/Actual frame decoded/});
  await frame.scrollIntoViewIfNeeded();
  await expect.poll(() => frame.evaluate((img: HTMLImageElement) => img.naturalWidth)).toBe(1078);
  const png = await page.request.get((await frame.getAttribute('src'))!);
  await proof.getByText('PNG SHA-256', {exact:true}).click();
  await expect(proof.locator('details code')).toHaveText(createHash('sha256').update(await png.body()).digest('hex'));
});
