import { expect } from '@playwright/test';
import { test } from './fixture-test';
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
    test(conditional ? 'uses a Chromium passkey from conditional autofill' : 'uses a Chromium passkey explicitly', async ({ context, page }) => {
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
        await page.getByRole('button', { name: 'Use a passkey' }).click();
      }
      await expect(page).toHaveURL('/dashboard#passkey');
      await expect(page.getByText('Signed in as test@example.invalid.')).toBeVisible();
    });
  }
});

test('Auth accepts trusted Hosting and direct HTTPS, not invalid forwarding headers', async ({ request, baseURL }) => {
  test.skip(fixtureOnly, 'Requires the local Google-only social Auth fixture');
  const edge = { 'sylphx-edge':'public', 'x-forwarded-proto':'https' };
  const nav = { 'sec-fetch-site':'same-origin', 'sec-fetch-mode':'navigate', 'sec-fetch-dest':'document' };
  for (const origin of [baseURL!, 'https://anymd.sylphx.com', 'https://web-5645d7.sylphx.app']) {
    const headers = origin === baseURL ? nav : {...nav, ...edge, 'x-forwarded-host':new URL(origin).host};
    const response = await request.get('/api/auth/google?next=/dashboard', {headers, maxRedirects:0});
    expect(response.status()).toBe(303);
    const location = new URL(response.headers().location);
    expect(location.origin + location.pathname).toBe('https://accounts.google.com/o/oauth2/v2/auth');
    const callback = new URL(location.searchParams.get('fixture_callback') ?? '');
    expect(callback.origin + callback.pathname).toBe(origin + '/api/auth/callback');
    expect(callback.searchParams.get('flow')).toMatch(/^[\w-]{43}$/);
    const session = await request.get('/api/auth/session?providers', {headers});
    expect(await session.json()).toMatchObject({providers:['google'], methods:{emailOtp:true, passkey:true}});
  }
  for (const headers of [
    {...edge, 'x-forwarded-host':'other-tenant.sylphx.app'},
    {...edge, 'x-forwarded-host':'anymd.sylphx.com, other-tenant.sylphx.app'},
    {'x-forwarded-host':'anymd.sylphx.com'},
    {...edge, 'x-forwarded-host':'anymd.sylphx.com', 'x-forwarded-proto':'http'},
  ]) {
    const config = await request.get('/api/auth/config', {headers});
    expect(config.status()).toBe(403);
    const response = await request.get('/api/auth/google?next=/dashboard', {headers:{...nav, ...headers}, maxRedirects:0});
    expect(response.status()).toBe(303);
    expect(response.headers().location).toBe('/sign-in?error=method_unavailable&next=%2Fdashboard');
    expect(response.headers()['set-cookie']).toBeUndefined();
    const session = await request.get('/api/auth/session?providers', {headers});
    expect(await session.json()).toMatchObject({providers:[], methods:{emailOtp:false, passkey:false}});
  }
  const github = await request.get('/api/auth/github', { maxRedirects: 0, headers:{
    'sec-fetch-site':'same-origin', 'sec-fetch-mode':'navigate', 'sec-fetch-dest':'document',
  } });
  expect(github.status()).toBe(303);
  expect(github.headers().location).toBe('/sign-in?error=method_unavailable&next=%2Fdashboard');
  expect(github.headers()['set-cookie']).toBeUndefined();
});

test('new SDK session is HttpOnly and sign-out revokes it', async ({ page, context }) => {
  test.skip(fixtureOnly, 'Requires the local Auth fixture');
  await page.goto('/sign-in');
  await page.getByRole('link', {name:'Continue with Google'}).click();
  await expect(page).toHaveURL('/dashboard');
  const credential = (await context.cookies()).find(cookie => cookie.name === '__Host-auth_session');
  expect(credential).toMatchObject({httpOnly:true, secure:true, sameSite:'Lax', path:'/'});
  await page.getByRole('button', {name:'Sign out', exact:true}).click();
  await expect(page).toHaveURL('/');
  expect((await context.cookies()).some(cookie => cookie.name === '__Host-auth_session')).toBe(false);
  const status = await page.request.get('/api/auth/session', {headers:{Cookie:credential!.name + '=' + credential!.value}});
  expect(await status.json()).toMatchObject({authConfigured:true, user:null});
});

test('legacy sessions still open the native CLI consent page', async ({ page, context, baseURL }) => {
  test.skip(fixtureOnly, 'Requires the local Auth fixture');
  await context.addCookies([{name:'anymd_session', value:'identity_org_session_fixture_legacy', url:baseURL!}]);
  const args = new URLSearchParams({port:'32101', state:'a'.repeat(32), challenge:'b'.repeat(64)});
  await page.goto('/cli-auth?' + args);
  await expect(page.getByText('test@example.invalid')).toBeVisible();
  await expect(page.getByRole('button', {name:'Connect this CLI'})).toBeVisible();
});

test('OAuth refuses a callback with no bound flow and issues no session', async ({ request }) => {
  const response = await request.get('/api/auth/callback?' + new URLSearchParams({flow:'a'.repeat(43)}), {maxRedirects:0});
  expect(response.status()).toBe(303);
  expect(response.headers().location).toContain('/sign-in?error=');
  expect(response.headersArray().filter(header => header.name.toLowerCase() === 'set-cookie').map(header => header.value)).not.toContainEqual(expect.stringMatching(/^__Host-auth_session=/));
});
