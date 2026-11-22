import { expect, test } from '@playwright/test';
import { generateKeyPairSync } from 'node:crypto';

const fixtureOnly = Boolean(process.env.PLAYWRIGHT_BASE_URL);

test('initial HTML shows only one entry with configured Google and email autocomplete', async ({ browser, baseURL }) => {
  const context = await browser.newContext({ javaScriptEnabled: false, baseURL });
  try {
    const page = await context.newPage();
    await page.goto('/sign-in?next=/billing');
    await expect(page.getByRole('heading', { name: 'Sign in to Anymd', exact: true })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Continue with Google' })).toHaveAttribute('href', '/api/auth/google?next=%2Fbilling');
    await expect(page.getByLabel('Email address')).toHaveAttribute('autocomplete', 'username webauthn');
    await expect(page.getByRole('button', { name: 'Continue', exact: true })).toBeVisible();
    await expect(page.getByText('Create an account')).toHaveCount(0);
    await expect(page.locator('input[type=password]')).toHaveCount(0);
  } finally { await context.close(); }
});

test('legacy aliases redirect permanently, preserving only the safe return path', async ({ request }) => {
  const next = '/cli-auth?client_id=desktop&state=example#details';
  for (const alias of ['login', 'signup', 'sign-up', 'register', 'forgot-password']) {
    const query = new URLSearchParams({ next });
    const res = await request.get('/' + alias + '?' + query, { maxRedirects: 0 });
    expect(res.status()).toBe(308);
    expect(res.headers().location).toBe('/sign-in?' + query);
  }
  const res = await request.get('/sign-up?next=https%3A%2F%2Fexample.invalid', { maxRedirects: 0 });
  expect(res.status()).toBe(308);
  expect(res.headers().location).toBe('/sign-in?next=%2Fdashboard');
});

test('with OTP and passkey disabled, only the existing-password fallback is offered on step 2', async ({ page }) => {
  test.skip(fixtureOnly, 'Requires the local Auth fixture');
  const attempts: string[] = [];
  page.on('request', req => { if (req.method() === 'POST') attempts.push(new URL(req.url()).pathname); });
  await page.goto('/sign-in');
  await expect(page.getByRole('button', { name: 'Use password instead' })).toHaveCount(0);
  await page.getByLabel('Email address').fill('unknown@example.invalid');
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Verify it’s you' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Use password instead' })).toBeVisible();
  await expect(page.locator('input[autocomplete=one-time-code]')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Use a passkey' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Send a code' })).toHaveCount(0);
  expect(attempts).toEqual([]);
  await page.getByRole('button', { name: 'Change email' }).click();
  await page.getByLabel('Email address').fill('test@example.invalid');
  await page.getByRole('button', { name: 'Continue', exact: true }).click();
  await page.getByRole('button', { name: 'Use password instead' }).click();
  await expect(page.getByLabel('Password')).toHaveAttribute('autocomplete', 'current-password');
  await page.getByLabel('Password').fill('wrong-password');
  await page.getByRole('button', { name: 'Sign in with password' }).click();
  await expect(page.getByRole('alert')).toContainText('Check your email and password');
  await page.getByLabel('Password').fill('fixture-long-password');
  await page.getByRole('button', { name: 'Sign in with password' }).click();
  await expect(page).toHaveURL('/dashboard');
  expect((await (await page.request.get('/api/auth/session')).json()).user).toMatchObject({ email: 'test@example.invalid' });
});

test('password-sign-up no longer exists at the app boundary', async ({ request, baseURL }) => {
  test.skip(fixtureOnly, 'Requires the local Auth fixture');
  const { origin } = new URL(baseURL!);
  const response = await request.post('/api/auth/password-sign-up', { headers: { Origin: origin }, data: { email: 'test@example.invalid' } });
  expect(response.status()).toBe(404);
});

test.describe('OTP and passkey enabled', () => {
  test.skip(fixtureOnly, 'Requires the local Auth fixture');
  test.use({ userAgent: 'anymd-test-otp-passkey' });

  test('submits six digits inline, keeps a rejected code, cools down resend, and redeems a session', async ({ page }) => {
    await page.goto('/sign-in?next=/dashboard%23signed-in');
    await page.getByLabel('Email address').fill('test@example.invalid');
    await page.getByRole('button', { name: 'Continue', exact: true }).click();
    await expect(page).toHaveURL(/\/sign-in\?next=/);
    const code = page.getByLabel('6-digit code');
    await expect(page.getByText('Sent to t•••@example.invalid')).toBeVisible();
    await expect(code).toHaveAttribute('autocomplete', 'one-time-code');
    await expect(code).toBeFocused();
    await expect(page.getByRole('button', { name: /^Resend/ })).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Use password instead' })).toBeVisible();
    await code.fill('234567');
    await expect(page.getByRole('alert')).toContainText('That code couldn’t be verified');
    await expect(code).toHaveValue('234567');
    await page.clock.install();
    await page.clock.fastForward(31_000);
    await expect(page.getByRole('button', { name: 'Resend', exact: true })).toBeEnabled();
    await code.fill('123 456');
    await expect(page).toHaveURL('/dashboard#signed-in');
    await expect(page.getByText('Signed in as test@example.invalid.')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Add a passkey' })).toBeVisible();
  });

  for (const conditional of [true, false]) {
    test(conditional ? 'uses a Chromium passkey from conditional autofill' : 'uses a Chromium passkey from step 2', async ({ context, page }) => {
      if (!conditional) await page.addInitScript(() => {
        PublicKeyCredential.isConditionalMediationAvailable = async () => false;
      });
      const cdp = await context.newCDPSession(page);
      await cdp.send('WebAuthn.enable');
      const { authenticatorId } = await cdp.send('WebAuthn.addVirtualAuthenticator', { options: {
        protocol: 'ctap2', transport: 'internal', hasResidentKey: true, hasUserVerification: true,
        isUserVerified: true, automaticPresenceSimulation: true,
      } });
      const { privateKey } = generateKeyPairSync('ec', { namedCurve: 'prime256v1' });
      await cdp.send('WebAuthn.addCredential', { authenticatorId, credential: {
        credentialId: Buffer.alloc(32, 1).toString('base64'), rpId: 'localhost', isResidentCredential: true,
        privateKey: privateKey.export({ format: 'der', type: 'pkcs8' }).toString('base64'),
        userHandle: Buffer.alloc(32, 2).toString('base64'), signCount: 0,
      } });
      await page.goto('/sign-in?next=/dashboard%23passkey');
      if (!conditional) {
        await page.getByLabel('Email address').fill('test@example.invalid');
        await page.getByRole('button', { name: 'Continue', exact: true }).click();
        await page.getByRole('button', { name: 'Use a passkey' }).click();
      }
      await expect(page).toHaveURL('/dashboard#passkey');
      await expect(page.getByText('Signed in as test@example.invalid.')).toBeVisible();
    });
  }
});

test('OAuth uses only approved callback origins and refuses disabled GitHub', async ({ request, baseURL }) => {
  test.skip(Boolean(process.env.PLAYWRIGHT_BASE_URL), 'Requires the local Google-only social Auth fixture');
  for (const [host, forwarded, origin] of [
    ['anymd-web.default.svc.cluster.local:3000', 'web-16b143.sylphx.app', 'https://web-16b143.sylphx.app'],
    ['anymd-web.default.svc.cluster.local:3000', '', baseURL],
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
