import { defineConfig } from '@playwright/test';

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
  webServer: process.env.PLAYWRIGHT_BASE_URL ? undefined : { command: 'bun run start', url: `${baseURL}/api/health`, timeout: 30_000 },
});
