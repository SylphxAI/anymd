import Script from 'next/script';

export function Diagnostics() {
  const key = process.env.ANYMD_OBS_PUBLISHABLE_KEY;
  const release = process.env.ANYMD_OBS_RELEASE;
  const url = process.env.ANYMD_OBS_CAPTURE_URL;
  if (!key || !release || !url) return null;
  // The runtime independently validates all three values and collects only
  // fixed failure fields on the canonical, non-account surfaces.
  return <Script src="/obs.js" strategy="beforeInteractive" data-capture-url={url} data-publishable-key={key} data-release={release} />;
}
