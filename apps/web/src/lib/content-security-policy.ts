const googleTag = 'https://www.googletagmanager.com';
const ads = [
  'https://www.google.com',
  'https://www.googleadservices.com',
  'https://googleads.g.doubleclick.net',
  'https://stats.g.doubleclick.net',
];
const analytics = ['https://www.google-analytics.com', 'https://region1.google-analytics.com'];

function httpsOrigin(value: string | undefined) {
  if (!value) return [];
  try {
    const url = new URL(value);
    return url.protocol === 'https:' && !url.username && !url.password ? [url.origin] : [];
  } catch { return []; }
}

export function contentSecurityPolicy(nonce: string) {
  const auth = httpsOrigin(process.env.SYLPHX_AUTH_URL);
  const money = httpsOrigin(process.env.ANYMD_MONEY_TEST_URL);
  const connect = [...new Set(["'self'", 'https://api.sylphx.com', ...auth, ...money, googleTag, ...analytics, ...ads])];
  return [
    "default-src 'none'", "base-uri 'none'", "object-src 'none'", "frame-ancestors 'none'",
    // Only the SSR nonce and known script origins can execute JavaScript.
    // WebAssembly is used by the first-party Studio, not by inline scripts.
    "script-src 'self' 'nonce-" + nonce + "' 'wasm-unsafe-eval' " + googleTag,
    "script-src-attr 'none'",
    // Solid/UI and Vite use inline styles (not scripts) for runtime presentation.
    "style-src 'self' 'unsafe-inline'", "font-src 'self'",
    "img-src 'self' data: blob: " + [googleTag, ...analytics, ...ads].join(' '),
    "media-src 'self' blob:", "worker-src 'self'", "manifest-src 'self'",
    'connect-src ' + connect.join(' '),
    "frame-src " + googleTag, "form-action 'self'" + (auth.length ? " " + auth.join(" ") : ""),
  ].join('; ');
}
