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

test('Google sign-in returns to billing with the existing account', async ({ page }) => {
  test.skip(fixtureOnly, 'Requires the local Auth fixture');
  await page.goto('/sign-in?next=/billing');
  await page.getByRole('link', {name:'Continue with Google'}).click();
  await expect(page).toHaveURL('/billing');
  expect((await (await page.request.get('/api/auth/session')).json()).user).toMatchObject({email:'test@example.invalid'});
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

test.describe('only social sign-in is available', () => {
  test.use({ userAgent: 'anymd-test-no-passwordless' });
  test('ignores the upstream password capability', async ({ page }) => {
    test.skip(fixtureOnly, 'Requires the local Auth fixture');
    await page.goto('/sign-in');
    await expect(page.getByRole('link', { name: 'Continue with Google' })).toBeVisible();
    await expect(page.getByText('Email sign-in is currently unavailable.')).toBeVisible();
    await expect(page.getByLabel('Email address')).toHaveCount(0);
    await expect(page.locator('input[type=password]')).toHaveCount(0);
  });
});

test('the app does not expose password sign-in or sign-up', async ({ request, baseURL }) => {
  test.skip(fixtureOnly, 'Requires the local Auth fixture');
  const { origin } = new URL(baseURL!);
  for (const endpoint of ['password-sign-in', 'password-sign-up']) {
    const response = await request.post('/api/auth/' + endpoint, { headers: { Origin: origin }, data: { email: 'test@example.invalid' } });
    expect(response.status()).toBe(404);
  }
});

test.describe('OTP and passkey enabled', () => {
  test.skip(fixtureOnly, 'Requires the local Auth fixture');

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
    await expect(page.locator('input[type=password]')).toHaveCount(0);
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
    ['web.fixture.svc.cluster.local:3000', 'anymd.sylphx.com', 'https://anymd.sylphx.com'],
    ['web.fixture.svc.cluster.local:3000', '', baseURL],
    ['anymd.sylphx.com', 'anymd.sylphx.com', 'https://anymd.sylphx.com'],
    ['web.fixture.svc.cluster.local:3000', 'preview.example.test', baseURL],
    ['evil.example', 'anymd.sylphx.com.evil.example', baseURL],
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
