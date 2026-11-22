export function parseCapabilities(body: { social?: unknown; methods?: Record<string, unknown> } | null = null) {
  const providers = ['google', 'github'].filter(provider => Array.isArray(body?.social) && body.social.some(
    entry => entry?.provider === provider && typeof entry.credentials === 'string' && entry.credentials.trim(),
  ));
  return { providers, methods: {
    emailOtp: body?.methods?.email_otp === true,
    passkey: body?.methods?.passkey === true,
  } };
}

export type Capabilities = ReturnType<typeof parseCapabilities>;
export const noCapabilities = parseCapabilities();
