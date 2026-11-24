import { test as base } from '@playwright/test';
import { fixtureApp, providerAuthorize } from './origins';

// Keep the SDK start/callback real, but replace the provider step in tests.
export const test = base.extend({
  page: async ({page}, use) => {
    if (!process.env.PLAYWRIGHT_BASE_URL) {
      await page.route(url => url.origin === fixtureApp && url.pathname === '/api/auth/google', async route => {
// Playwright drops browser-only navigation headers when replaying with route.fetch.
        const start = await route.fetch({maxRedirects:0, headers:{...await route.request().allHeaders(), 'sec-fetch-site':'same-origin', 'sec-fetch-mode':'navigate', 'sec-fetch-dest':'document'}});
        const target = new URL(start.headers().location ?? '/', fixtureApp);
        if (target.origin + target.pathname !== providerAuthorize) return route.fulfill({response:start});
        const callback = target.searchParams.get('fixture_callback');
        if (!callback || new URL(callback).origin !== fixtureApp) return route.abort();
        return route.fulfill({response:start, headers:{...start.headers(), location:callback}});
      });
    }
    await use(page);
  },
});
