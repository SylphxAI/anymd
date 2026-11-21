import { expect, test } from '@playwright/test';
test.describe('server-rendered Google and password sign-in', () => {
  test.use({ javaScriptEnabled: false });
  test('renders Google in initial HTML and hides disabled methods', async ({ page }) => {
    await page.goto('/sign-in?next=/billing');
    await expect(page.getByRole('link', { name: 'Continue with Google' })).toHaveAttribute('href', '/api/auth/google?next=%2Fbilling');
    await expect(page.getByRole('link', { name: 'Continue with GitHub' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Use a passkey' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Send sign-in code' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Sign in with password' })).toBeVisible();
    await expect(page.getByText('or use email')).toBeVisible();
  });
});

test.describe('OTP and passkey Auth configuration', () => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires the local Auth fixture');
  test.use({ userAgent: 'anymd-test-otp-passkey' });
  test('shows enabled methods without password or a social divider', async ({ context, page }) => {
    await page.goto('/sign-in');
    await expect(page.getByRole('button', { name: 'Use a passkey' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Send sign-in code' })).toBeVisible();
    await expect(page.getByLabel('Password')).toHaveCount(0);
    await expect(page.getByText('or use email')).toHaveCount(0);
    const token = process.env.ANYMD_E2E_SESSION;
    if (!token) throw new Error('Missing local test session');
    await context.addCookies([{ name: 'anymd_session', value: token, url: new URL(page.url()).origin }]);
    await page.goto('/dashboard');
    await expect(page.getByRole('button', { name: 'Add a passkey' })).toBeVisible();
  });
});

test('password sign-up is non-enumerating and sign-in redeems a server ticket', async ({ page }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires the local Auth fixture');
  await page.goto('/sign-in');
  await page.getByRole('button', { name: 'Create an account' }).click();
  await expect(page.getByLabel('Password')).toHaveAttribute('autocomplete', 'new-password');
  await page.getByLabel('Email address').fill('test@example.invalid');
  await page.getByLabel('Password').fill('fixture-long-password');
  await page.getByRole('button', { name: 'Create account' }).click();
  await expect(page.getByText('Check your inbox', { exact: false })).toBeVisible();
  expect((await (await page.request.get('/api/auth/session')).json()).user).toBeNull();
  await page.getByLabel('Password').fill('wrong-password');
  await page.getByRole('button', { name: 'Sign in with password' }).click();
  await expect(page.getByRole('alert').filter({ hasText: 'Check your email and password' })).toBeVisible();
  await page.getByLabel('Password').fill('fixture-long-password');
  await page.getByRole('button', { name: 'Sign in with password' }).click();
  await expect(page).toHaveURL('/dashboard');
  await expect(page.getByText('Signed in as test@example.invalid.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Add a passkey' })).toHaveCount(0);
  expect(await (await page.request.get('/api/auth/session?providers')).json()).toMatchObject({
    authConfigured: true, providers: ['google'], methods: { emailOtp: false, passkey: false, password: true, signUp: true },
  });
});

test('OAuth uses only approved callback origins and refuses disabled GitHub', async ({ request, baseURL }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires the local Google-only social Auth fixture');
  for (const [host, forwarded, origin] of [
    ['0.0.0.0:3000', 'web-16b143.sylphx.app', 'https://web-16b143.sylphx.app'],
    ['anymd.sylphx.com', 'anymd.sylphx.com', 'https://anymd.sylphx.com'],
    ['evil.example', 'web-16b143.sylphx.app.evil.example', baseURL],
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
