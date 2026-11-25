import { expect, test } from '@playwright/test';

const icons = [
  {src:'/icon-192.png', sizes:'192x192', purpose:'any', type:'image/png'},
  {src:'/icon-512.png', sizes:'512x512', purpose:'any', type:'image/png'},
  {src:'/icon-maskable-192.png', sizes:'192x192', purpose:'maskable', type:'image/png'},
  {src:'/icon-maskable-512.png', sizes:'512x512', purpose:'maskable', type:'image/png'},
];

test('Chromium loads the branded standalone manifest under enforcing CSP without a service worker', async ({ page, request }) => {
  await page.route('**/cdn-cgi/trace', route => route.fulfill({body:'loc=DE\n'}));
  await page.addInitScript(() => {
    const violations: string[] = [];
    document.addEventListener('securitypolicyviolation', event => violations.push(event.effectiveDirective));
    Reflect.set(window, '__manifestViolations', violations);
  });
  const result = await page.goto('/studio');
  const directives = (await result?.headerValue('content-security-policy'))?.split('; ');
  expect(directives).toEqual(expect.arrayContaining(["default-src 'none'", "manifest-src 'self'"]));
  await expect(page.locator('head link[rel=manifest]')).toHaveAttribute('href', '/manifest.webmanifest');
  await expect(page.locator('head meta[name=theme-color]')).toHaveAttribute('content', '#C3F53C');
  await expect(page.locator('head link[rel=apple-touch-icon]')).toHaveAttribute('href', '/apple-touch-icon-180.png');
  const manifestResponse = await request.get('/manifest.webmanifest');
  expect(manifestResponse.status()).toBe(200);
  expect(manifestResponse.headers()['content-type']).toMatch(/application\/(?:manifest\+)?json/);
  const manifest = await manifestResponse.json();
  expect(manifest).toEqual({
    name:'Anymd', short_name:'Anymd', description:expect.any(String), start_url:'/studio', scope:'/',
    display:'standalone', background_color:'#FBFCF8', theme_color:'#C3F53C', icons,
  });
  for (const {src, sizes} of [...icons, {src:'/apple-touch-icon-180.png', sizes:'180x180'}]) {
    const response = await request.get(src);
    expect(response.status(), src).toBe(200);
    expect(response.headers()['content-type']).toContain('image/png');
    // PNG IHDR stores the actual width and height at offsets 16 and 20.
    const bytes = await response.body();
    expect([bytes.readUInt32BE(16), bytes.readUInt32BE(20)].join('x')).toBe(sizes);
  }
  const browser = await page.context().newCDPSession(page);
  const parsed = await browser.send('Page.getAppManifest');
  expect(parsed.errors).toEqual([]);
  expect(parsed.url).toBe(new URL('/manifest.webmanifest', page.url()).href);
  expect(JSON.parse(parsed.data ?? '{}')).toEqual(manifest);
  expect(await page.evaluate(() => Reflect.get(window, '__manifestViolations'))).toEqual([]);
  expect(await page.evaluate(async () => (await navigator.serviceWorker.getRegistrations()).length)).toBe(0);
});
