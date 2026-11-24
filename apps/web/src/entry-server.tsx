import { randomBytes } from 'node:crypto';
import { contentSecurityPolicy } from './lib/content-security-policy';
import { resolveEffectiveConsent } from '@sylphx/sdk/signal';
import { getRequestEvent } from 'solid-js/web';
import { createHandler, StartServer } from '@solidjs/start/server';
import { localeFromPath } from './lib/marketing-locale';

export default createHandler(() => <StartServer document={({ assets, children, scripts }) => {
  const key = process.env.ANYMD_OBS_PUBLISHABLE_KEY;
  const release = process.env.ANYMD_OBS_RELEASE;
  const url = process.env.ANYMD_OBS_CAPTURE_URL;
  const request = getRequestEvent()?.request;
  const locale = localeFromPath(request ? new URL(request.url).pathname : '/');
  const gpc = resolveEffectiveConsent({storedChoice:'granted'}, request?.headers).analytics_storage === 'denied';
  return <html lang={locale} data-signal-gpc={gpc ? '1' : undefined}>
    <head>
      <meta charset="utf-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1" />
      <link rel="icon" href="/favicon.svg" />
      {key && release && url && <script src="/obs.js" data-capture-url={url} data-publishable-key={key} data-release={release} />}
      {assets}
    </head>
    <body><div id="app">{children}</div>{scripts}</body>
  </html>;
}} />, (event) => {
  const nonce = randomBytes(16).toString('base64');
  // The Vite development client needs a WebSocket and injected styles. Test
  // and serve the actual enforced policy from the production build.
  if (import.meta.env.PROD) event.response.headers.set('Content-Security-Policy', contentSecurityPolicy(nonce));
  return { mode: 'async', nonce };
});
