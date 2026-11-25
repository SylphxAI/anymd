import { expect, test } from '@playwright/test';

test('health reports Money without sign-in or the public Host header', async ({ request, baseURL }) => {
  for (const headers of [undefined, { Host: '127.0.0.1:3000' }]) {
    // The alternate header models a probe hitting the pod directly, not a CDN route.
    if (headers && !baseURL?.startsWith('https://localhost:')) continue;
    const response = await request.get('/api/health', { headers });
    expect(response.status()).toBe(200);
    expect(response.headers()['cache-control']).toBe('private, no-store');
    expect(await response.json()).toEqual({ status: 'ok', service: 'anymd-web', money: 'ok' });
  }
});
