import type { APIEvent } from '@solidjs/start/server';
import { authConfig, session } from '@/lib/auth';
import { entryOptions, handleAuth, noEntry } from '@/lib/auth-handler';
import { authRoute } from '@/lib/auth-route';
import { failure, json, redirect, safeNext } from '@/lib/http';

async function auth({request}: APIEvent) {
  const url = new URL(request.url);
  const config = authConfig();
  try {
    // Public account projection also consumed by the native CLI.
    if (request.method === 'GET' && url.pathname === authRoute + '/session') {
      const active = config && await session(request, config);
      const entry = url.searchParams.has('providers') ? config ? await entryOptions(request, config) : noEntry : null;
      return json({
        authConfigured: Boolean(config),
        providers: entry?.providers ?? [],
        ...(entry && {methods:{emailOtp:entry.otp, passkey:entry.passkey}}),
        user: active ? {name:active.user.name, email:active.user.email} : null,
      });
    }
    const social = request.method === 'GET' && [authRoute + '/google', authRoute + '/github'].includes(url.pathname);
    if (social || (!config && request.method === 'GET' && url.pathname === authRoute + '/callback')) {
      const response = config && await handleAuth(request, config);
      if (response && response.status === 303) return response;
      const next = safeNext(url.searchParams.get('next'));
      return redirect('/sign-in?' + new URLSearchParams({error:config ? 'method_unavailable' : 'auth_unconfigured', next}));
    }
    return config ? await handleAuth(request, config) : json({error:'auth_unconfigured'}, 503);
  } catch (err) { return failure(err); }
}

export { auth as GET, auth as POST };
