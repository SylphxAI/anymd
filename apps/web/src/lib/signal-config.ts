// Runtime binding: only the environment's public key and exact HTTPS origin
// may be serialized into HTML. Bad explicit configuration disables Signal.
export function signalConfig(env = process.env) {
  const apiKey = env.SYLPHX_PUBLISHABLE_KEY?.trim();
  if (!apiKey?.startsWith('sylphx_pk_')) return undefined;
  const raw = env.SYLPHX_API_URL?.trim() || 'https://api.sylphx.com';
  try {
    const url = new URL(raw);
    if (url.protocol === 'https:' && (raw === url.origin || raw === url.origin + '/')) {
      return {apiKey, baseUrl:url.origin};
    }
  } catch { /* optional measurement fails closed */ }
}
