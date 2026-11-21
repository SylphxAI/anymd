import { expect, test } from '@playwright/test';
test('sign-in hides GitHub when only Google is usable', async ({ page }) => {
  await page.goto('/sign-in');
  await expect(page.getByRole('link', { name: 'Continue with Google' })).toBeVisible();
  await expect(page.getByText('Continue with GitHub')).toHaveCount(0);
});
test('OAuth uses only approved callback origins and refuses disabled GitHub', async ({ request }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires the local Google-only Auth fixture');
  for (const [host, forwarded, origin] of [
    ['0.0.0.0:3000', 'web-16b143.sylphx.app', 'https://web-16b143.sylphx.app'],
    ['anymd.sylphx.com', 'anymd.sylphx.com', 'https://anymd.sylphx.com'],
    ['evil.example', 'web-16b143.sylphx.app.evil.example', 'https://anymd.sylphx.com'],
  ]) {
    const response = await request.get('/api/auth/google?next=/dashboard', { headers: { host, 'x-forwarded-host': forwarded }, maxRedirects: 0 });
    expect(response.status()).toBe(303);
    const location = new URL(response.headers().location);
    expect(location.pathname).toBe('/v1/client/oauth/google/start');
    const callback = new URL(location.searchParams.get('redirect_url') ?? '');
    expect(callback.origin + callback.pathname).toBe(origin + '/api/auth/callback');
    expect(callback.searchParams.get('state')).toBeTruthy();
  }
  const github = await request.get('/api/auth/github', { maxRedirects: 0 });
  expect(github.status()).toBe(303);
  expect(github.headers().location).toBe('/sign-in?error=provider_unavailable');
  expect(github.headers()['set-cookie']).toBeUndefined();
});
