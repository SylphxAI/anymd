import { serve } from 'bun';
import { randomBytes } from 'node:crypto';
import { fixtureApp, fixtureAuth, fixturePublishable, fixtureSecret, fixtureTenant, providerAuthorize } from './origins';

const tickets = new Map<string, {is_new_user?: boolean}>();
function issueTicket(request: Request) {
  const ticket = randomBytes(24).toString('base64url');
  const scenario = request.headers.get('user-agent');
  tickets.set(ticket, scenario === 'anymd-fixture-new-user' ? {is_new_user:true}
    : scenario === 'anymd-fixture-returning-user' ? {is_new_user:false} : {});
  return ticket;
}
const activeSessions = new Set<string>();
const legacySession = 'identity_org_session_fixture_legacy';
activeSessions.add(legacySession);
const flow = randomBytes(32).toString('base64url');
const challenges = new Map<string, string>();
const checkoutMetadata = new Map<string, unknown>();
const fixtureUser = 'usr_fixture001';
serve({ hostname: '127.0.0.1', port: 3031, tls:{cert:process.env.NITRO_SSL_CERT, key:process.env.NITRO_SSL_KEY}, async fetch(request: Request) {
  const url = new URL(request.url);
  if (url.pathname === '/health') return new Response('ok');
  if (url.pathname === '/test/checkout-metadata') return Response.json(checkoutMetadata.get(url.searchParams.get('intent') ?? '') ?? null);
  if (url.pathname === '/v1/client/oauth/google/start' && url.searchParams.get('session_mode') === 'server' &&
      request.headers.get('authorization') === 'Bearer ' + fixturePublishable) {
    const callback = new URL(url.searchParams.get('redirect_url')!);
    if (![fixtureApp, 'https://anymd.sylphx.com', 'https://web-5645d7.sylphx.app'].includes(callback.origin) || callback.pathname !== '/api/auth/callback') return new Response(null, {status:400});
    callback.searchParams.set('auth_ticket', issueTicket(request));
    return Response.redirect(providerAuthorize + '?' + new URLSearchParams({fixture_callback:callback.href}), 303);
  }
  const checkoutCollection = '/v1/orgs/org_fixture/projects/prj_fixture/envs/env_fixture/checkout_sessions';
  if (url.pathname.startsWith(checkoutCollection) && request.headers.get('authorization') === 'Bearer ' + ['sylphx', 'sk', 'test', 'fixture'].join('_')) {
    const name = checkoutCollection.slice(4) + '/chk-111111111111111111111111';
    const processor_session_id = 'cs_test_Fixture123';
    if (request.method === 'POST' && url.pathname === checkoutCollection && !url.search) {
      const data = await request.json();
      const prefix = `anymd-${fixtureUser}-`;
      const key = request.headers.get('idempotency-key');
      const intent = key?.startsWith(prefix) ? key.slice(prefix.length) : '';
      if (data.subject?.end_user === fixtureUser && data.line_items?.[0]?.price === 'anymd_pro' &&
          new URL(data.success_url).searchParams.get('session_id') === '{CHECKOUT_SESSION_ID}' && /^[a-f0-9]{64}$/.test(intent)) {
        checkoutMetadata.set(intent, data.metadata);
        return Response.json({name, processor_session_id, url:'https://checkout.stripe.com/c/pay/' + processor_session_id});
      }
    }
  }

  const instance = {id:fixtureTenant, slug:'anymd-fixture', environment:'test'};
  if (url.pathname === '/v1/client/config' && request.headers.get('authorization') === 'Bearer ' + fixturePublishable) {
    return Response.json(request.headers.get('user-agent') !== 'anymd-test-no-passwordless' ? {
      instance,
      social: [{ provider: 'google', credentials: 'shared' }, {provider:'apple', credentials:'shared'}, {provider:'github', credentials:'custom', start_url:fixtureAuth + '/oauth/github/start'}],
      methods: { passkey: true, email_otp: true, password: true, sign_up: true },
    } : {
      instance,
      social: [{ provider: 'google', credentials: 'shared' }, {provider:'apple', credentials:'shared'}, {provider:'github', credentials:'custom', start_url:fixtureAuth + '/oauth/github/start'}],
      methods: { passkey: false, email_otp: false, password: true, sign_up: true },
    });
  }
  const presented = request.headers.get('authorization')?.slice('Bearer '.length) ?? '';
  if (url.pathname === '/v1/sessions/current' && request.headers.get('x-sylphx-caller-key') === fixturePublishable) {
    if (!activeSessions.has(presented)) return Response.json({code:'unauthorized'}, {status:401});
    return Response.json({ session: { principal: {
      project_id: fixtureTenant, principal_id: fixtureUser, primary_email: 'test@example.invalid', state: 'active',
    } } });
  }
  if (request.method === 'POST' && url.pathname === '/v1/client/sign-out') {
    // The shared legacy fixture is read by concurrent test projects.
    if (presented !== legacySession) activeSessions.delete(presented);
    return new Response(null, {status:204});
  }
  if (request.method === 'POST' && url.pathname === '/v1/client/tickets:redeem' && request.headers.get('authorization') === 'Bearer ' + fixtureSecret) {
    const {ticket} = await request.json();
    const signup = tickets.get(ticket);
    if (!signup) return new Response(null, { status:401 });
    tickets.delete(ticket);
    const token = randomBytes(24).toString('base64url');
    activeSessions.add(token);
    return Response.json({session:{token, expires_at:Math.floor(Date.now()/1000)+3600}, user:{id:fixtureUser, email_verified:true}, ...signup});
  }
  if (request.method === 'POST' && request.headers.get('authorization') === 'Bearer ' + fixturePublishable) {
    const body = await request.json();
    if (request.headers.get('user-agent') !== 'anymd-test-no-passwordless') {
      if (url.pathname === '/v1/client/continue' && /^[^@]+@example\.invalid$/.test(body.email) &&
          body.method === 'email_otp' && body.session_mode === 'server') {
        return Response.json({ step: 'email_otp', flow, expires_in: 600 });
      }
      if (url.pathname === '/v1/client/continue/verify' && body.flow === flow && body.code === '123456') return Response.json({ticket:issueTicket(request)});
      if (url.pathname === '/v1/client/sign-in/passkey/begin' && body.rp_id === 'localhost' && body.origin === fixtureApp) {
        const id = randomBytes(16).toString('base64url');
        const challenge = randomBytes(32).toString('base64url');
        challenges.set(id, challenge);
        return Response.json({ challenge_id: id, challenge, rp_id: body.rp_id, allowed_credential_ids: [] });
      }
      if (url.pathname === '/v1/client/sign-in/passkey/verify' && body.session_mode === 'server') {
        const challenge = challenges.get(body.challenge_id);
        const data = JSON.parse(Buffer.from(body.client_data_json, 'base64url').toString());
        if (challenge && data.challenge === challenge && data.origin === fixtureApp &&
            data.type === 'webauthn.get' && body.credential_id && body.signature && body.authenticator_data) {
          challenges.delete(body.challenge_id);
          return Response.json({ticket:issueTicket(request)});
        }
      }
    }
    return Response.json({ code: 'auth_refused' }, { status: 401 });
  }
  return new Response(null, { status: 404 });
} });
