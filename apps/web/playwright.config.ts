import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { defineConfig } from '@playwright/test';
import { fixtureAdsId, fixtureAdsLabel, fixtureApp, fixtureAuth, fixtureMoneyParent, fixturePublishable, fixtureSecret, fixtureTenant } from './e2e/origins';

const live = process.env.PLAYWRIGHT_BASE_URL;
const tls = live ? undefined : loopbackTls();
function loopbackTls() {
  // The SDK requires HTTPS and Secure cookies. Trust only this run's local cert.
  const dir = mkdtempSync(join(tmpdir(), 'anymd-playwright-'));
  process.on('exit', () => rmSync(dir, {recursive:true, force:true}));
  const cert = join(dir, 'cert.pem');
  const key = join(dir, 'key.pem');
  execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
    '-subj', '/CN=localhost', '-addext', 'subjectAltName=DNS:localhost,IP:127.0.0.1', '-keyout', key, '-out', cert], {stdio:'ignore'});
  return {NITRO_SSL_CERT:readFileSync(cert, 'utf8'), NITRO_SSL_KEY:readFileSync(key, 'utf8'), NODE_EXTRA_CA_CERTS:cert};
}

const baseURL = live || fixtureApp;
export default defineConfig({
  testDir: './e2e',
  use: { baseURL, ignoreHTTPSErrors: !live, launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_PATH }, trace: 'retain-on-failure' },
  projects: [
    ...(['light', 'dark'] as const).flatMap((colorScheme) => [
      { name: `desktop-${colorScheme}`, use: { colorScheme, viewport: { width: 1440, height: 900 } } },
      { name: `mobile-${colorScheme}`, use: { colorScheme, viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true } },
    ]),
  ],
  webServer: live ? undefined : [
    { command: 'bun e2e/auth-fixture.ts', url: fixtureAuth + '/health', ignoreHTTPSErrors: true, env: tls },
    { command: 'bun run start', url: baseURL + '/api/health', timeout: 30_000, ignoreHTTPSErrors: true, env: {
      ...tls,
      ANYMD_APP_ORIGIN: baseURL, SYLPHX_AUTH_URL: fixtureAuth, SYLPHX_API_URL: fixtureAuth,
      ANYMD_MONEY_TEST_URL: fixtureAuth, ANYMD_MONEY_TEST_PARENT: fixtureMoneyParent,
      ANYMD_MONEY_TEST_KEY: ['sylphx', 'pk', 'test', 'fixture'].join('_'),
      ANYMD_MONEY_TEST_API_KEY: ['sylphx', 'sk', 'test', 'fixture'].join('_'),
      SYLPHX_PUBLISHABLE_KEY: fixturePublishable, SYLPHX_AUTH_SECRET_KEY: fixtureSecret,
      SYLPHX_AUTH_ORGANIZATION_ID: fixtureTenant,
      ANYMD_GOOGLE_ADS_ID: fixtureAdsId, ANYMD_GOOGLE_ADS_PURCHASE_LABEL: fixtureAdsLabel,
    } },
  ],
});
