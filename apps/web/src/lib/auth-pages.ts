import { query } from '@solidjs/router';
import { noCapabilities } from './auth-capabilities';

export const signInOptions = query(async () => {
  'use server';
  const { getWebRequest } = await import('@solidjs/start/http');
  const { authConfig, clientCapabilities } = await import('./auth');
  const config = authConfig();
  const capabilities = config ? await clientCapabilities(getWebRequest(), config).catch(() => noCapabilities) : noCapabilities;
  return { authConfigured: Boolean(config), ...capabilities };
}, 'sign-in-options');

export const cliAccount = query(async () => {
  'use server';
  const { getWebRequest } = await import('@solidjs/start/http');
  const { authConfig, session } = await import('./auth');
  const config = authConfig();
  const active = config ? await session(getWebRequest(), config).catch(() => null) : null;
  // Serialize only the display label, never the Auth session or credentials.
  return { configured: Boolean(config), label: active ? active.user.email || active.user.name || active.user.id : null };
}, 'cli-account');
