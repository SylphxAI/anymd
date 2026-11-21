import { defineConfig } from '@playwright/test';

const authOrigin = 'http://127.0.0.1:3031';

const baseURL = process.env.PLAYWRIGHT_BASE_URL || 'http://localhost:3000';
export default defineConfig({
  testDir: './e2e',
  use: { baseURL, launchOptions: { executablePath: process.env.PLAYWRIGHT_CHROMIUM_PATH }, trace: 'retain-on-failure' },
  projects: [
    ...(['light', 'dark'] as const).flatMap((colorScheme) => [
      { name: `desktop-${colorScheme}`, use: { colorScheme, viewport: { width: 1440, height: 757 } } },
      { name: `mobile-${colorScheme}`, use: { colorScheme, viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true } },
    ]),
  ],
  webServer: process.env.PLAYWRIGHT_BASE_URL ? undefined : [
    { command: 'bun e2e/auth-fixture.ts', url: authOrigin + '/health' },
    { command: 'bun run start', url: baseURL + '/api/health', timeout: 30_000, env: {
      ANYMD_APP_ORIGIN: baseURL, SYLPHX_AUTH_URL: authOrigin,
      SYLPHX_PUBLISHABLE_KEY: 'fixture', SYLPHX_AUTH_SECRET_KEY: 'fixture',
      SYLPHX_AUTH_ORGANIZATION_ID: '00000000-0000-0000-0000-000000000000',
    } },
  ],
});
