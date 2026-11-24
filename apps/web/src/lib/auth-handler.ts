import { getCookie, setCookie } from '@solidjs/start/http';
import { createSylphxAuthHandler } from '@sylphx/auth';
import type { EntryConfig } from '@sylphx/auth/client';
import { type Config, authTenantId, legacySessionCookies, session } from './auth';
import { consentKey } from './consent';
import { appOrigin, safeNext } from './http';
import { authRoute } from './auth-route';
import { signupCookie, signupSignal, type SignupSignal } from './signup';

export const noEntry: EntryConfig = { providers: [], password: false, signup: false, otp: false, passkey: false };

export async function handleAuth(request: Request, config: Config) {
  const origin = appOrigin(request);
  if (!origin.startsWith('https:')) return new Response(null, {status:503});
  let signup: SignupSignal | undefined;
  const handle = createSylphxAuthHandler({
    origin,
    authApiOrigin: config.origin,
    tenantAuthOrigin: 'https://auth.anymd.sylphx.com',
    publishableKey: config.pk,
    async fetch(url, init) {
      const response = await fetch(url, init);
      if (url.origin === config.origin && url.pathname === '/v1/client/config' && response.ok) {
        const { instance } = await response.clone().json();
        if (typeof instance?.id !== 'string' || authTenantId(instance.id) !== config.organization) {
          return Response.json({}, {status:403});
        }
      }
      return response;
    },
    sessionMode: 'server',
    sessionSecret: config.sk,
    legacyCookies: legacySessionCookies,
    routePrefix: authRoute,
    loginPath: '/sign-in',
    defaultReturnTo: '/dashboard',
    allowReturnTo: path => safeNext(path) === path,
    trustSylphxEdge: true,
    allowedMethods: ['oauth', 'otp', 'passkey'],
    providers: ['google', 'github'],
    directOAuth: true,
    passkeyEnrollment: async incoming => (await session(incoming, config))?.token,
    signUpSignal({method, provider, returnTo}, incoming) {
      const signupMethod = method === 'otp' ? 'email_otp' : method === 'passkey' ? method : provider;
      if (signupMethod === 'email_otp' || signupMethod === 'passkey' || signupMethod === 'google' || signupMethod === 'github') {
        signup = signupSignal(true, getCookie(consentKey), incoming.headers, signupMethod, returnTo);
      }
      return !!signup;
    },
  });
  const response = await handle(request);
  if (signup && response.headers.has('X-Sylphx-Auth-Event')) {
    response.headers.delete('X-Sylphx-Auth-Event');
    setCookie(signupCookie, signup.method + '.' + signup.entry_surface, {path:'/', maxAge:60, secure:true, sameSite:'lax'});
  }
  return response;
}

export async function entryOptions(request: Request, config: Config): Promise<EntryConfig> {
  // This server-side read keeps the transport authority, not the browser's navigation context.
  const headers = new Headers(request.headers);
  headers.delete('origin');
  headers.delete('sec-fetch-site');
  const response = await handleAuth(new Request(new URL(authRoute + '/config', request.url), {headers, signal:request.signal}), config);
  return response.ok ? response.json() : noEntry;
}
