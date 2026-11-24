import { query } from '@solidjs/router';

export const signInOptions = query(async () => {
  'use server';
  const { getWebRequest } = await import('@solidjs/start/http');
  const { authConfig } = await import('./auth');
  const { entryOptions, noEntry } = await import('./auth-handler');
  const config = authConfig();
  const entry = config ? await entryOptions(getWebRequest(), config).catch(() => noEntry) : noEntry;
  return { authConfigured: Boolean(config), entry };
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
