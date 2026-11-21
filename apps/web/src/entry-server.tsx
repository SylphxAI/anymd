import { createHandler, StartServer } from '@solidjs/start/server';

export default createHandler(() => <StartServer document={({ assets, children, scripts }) => {
  const key = process.env.ANYMD_OBS_PUBLISHABLE_KEY;
  const release = process.env.ANYMD_OBS_RELEASE;
  const url = process.env.ANYMD_OBS_CAPTURE_URL;
  return <html lang="en">
    <head>
      <meta charset="utf-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1" />
      <link rel="icon" href="/favicon.svg" />
      {key && release && url && <script src="/obs.js" data-capture-url={url} data-publishable-key={key} data-release={release} />}
      {assets}
    </head>
    <body><div id="app">{children}</div>{scripts}</body>
  </html>;
}} />, { mode: 'async' });
