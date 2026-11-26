import { expect, test } from '@playwright/test';

for (const [path, status] of [['/', 200], ['/api/health', 200], ['/favicon.ico', 200], ['/hsts-test-not-found', 404], ['/pricing.html', 301]] as const) {
  test('serves HSTS on ' + path, async ({ request }) => {
    const response = await request.get(path, { maxRedirects: 0 });
    expect(response.status()).toBe(status);
    expect(response.headers()['strict-transport-security']).toBe('max-age=63072000; includeSubDomains');
  });
}
