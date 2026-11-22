import { serve } from 'bun';
import { randomBytes } from 'node:crypto';

const ticket = randomBytes(24).toString('base64url');
const token = randomBytes(24).toString('base64url');
const flow = randomBytes(32).toString('base64url');
const challenges = new Map<string, string>();
const bindings = new Map<string, { body: unknown; responseStarted: boolean }>();
serve({ hostname: '127.0.0.1', port: 3031, async fetch(request: Request) {
  const url = new URL(request.url);
  if (url.pathname === '/health') return new Response('ok');
  if (url.pathname === '/test/attribution') return Response.json(bindings.get(url.searchParams.get('client') ?? '') ?? null);
  if (url.pathname === '/v1/events/attribution_bindings' && request.method === 'POST' &&
      request.headers.get('x-sylphx-key') === ['sylphx', 'pk', 'test', 'fixture'].join('_')) {
    const body = await request.json();
    if (typeof body.ga_client_id !== 'string') return new Response(null, {status:400});
    const binding = {body, responseStarted:false};
    bindings.set(body.ga_client_id, binding);
    // Reject before the BFF's deadline. If the route awaited Events, checkout
    // could not finish while this response is still in flight.
    await Bun.sleep(650);
    binding.responseStarted = true;
    return new Response('fixture upstream failure', {status:503});
  }
  if (url.pathname === '/v1/client/oauth/google/start' && url.searchParams.get('session_mode') === 'server') {
    const callback = new URL(url.searchParams.get('redirect_url')!);
    if (callback.origin !== 'http://localhost:3000' || callback.pathname !== '/api/auth/callback') return new Response(null, {status:400});
    callback.searchParams.set('sylphx_ticket', ticket);
    return Response.redirect(callback);
  }
  const checkoutCollection = '/v1/orgs/org_fixture/projects/prj_fixture/envs/env_fixture/checkout_sessions';
  if (url.pathname.startsWith(checkoutCollection) && request.headers.get('authorization') === 'Bearer ' + ['sylphx', 'sk', 'test', 'fixture'].join('_')) {
    const name = checkoutCollection.slice(4) + '/chk-111111111111111111111111';
    const processor_session_id = 'cs_test_Fixture123';
    if (request.method === 'POST' && url.pathname === checkoutCollection && !url.search) {
      const data = await request.json();
      if (data.subject?.end_user === 'usr_fixture001' && data.line_items?.[0]?.price === 'anymd_pro' &&
          new URL(data.success_url).searchParams.get('session_id') === '{CHECKOUT_SESSION_ID}' && request.headers.has('idempotency-key')) {
        return Response.json({name, processor_session_id, url:'https://checkout.stripe.com/c/pay/' + processor_session_id});
      }
    }
  }

  if (url.pathname === '/v1/client/config' && url.searchParams.get('publishable_key') === 'pk_fixture' && request.headers.get('authorization') === 'Bearer pk_fixture') {
    return Response.json(request.headers.get('user-agent') !== 'anymd-test-no-passwordless' ? {
      social: [{ provider: 'google', credentials: 'shared' }],
      methods: { passkey: true, email_otp: true, password: true, sign_up: true },
    } : {
      social: [{ provider: 'google', credentials: 'shared' }],
      methods: { passkey: false, email_otp: false, password: true, sign_up: true },
    });
  }
  if (url.pathname === '/v1/sessions/current' && request.headers.get('authorization') === 'Bearer ' + token &&
      request.headers.get('x-sylphx-caller-key') === 'pk_fixture') {
    return Response.json({ session: { principal: {
      project_id: '00000000-0000-0000-0000-000000000000', principal_id: 'usr_fixture001', primary_email: 'test@example.invalid', state: 'active',
    } } });
  }
  if (request.method === 'POST' && url.pathname === '/v1/client/tickets:redeem' && request.headers.get('authorization') === 'Bearer sk_fixture') {
    return (await request.json()).ticket === ticket ? Response.json({ session: { token } }) : new Response(null, { status: 401 });
  }
  if (request.method === 'POST' && request.headers.get('authorization') === 'Bearer pk_fixture') {
    const body = await request.json();
    if (request.headers.get('user-agent') !== 'anymd-test-no-passwordless') {
      if (url.pathname === '/v1/client/continue' && /^[^@]+@example\.invalid$/.test(body.email) &&
          body.method === 'email_otp' && body.session_mode === 'server') {
        return Response.json({ step: 'email_otp', flow, expires_in: 600 });
      }
      if (url.pathname === '/v1/client/continue/verify' && body.flow === flow && body.code === '123456') return Response.json({ ticket });
      if (url.pathname === '/v1/client/sign-in/passkey/begin' && body.rp_id === 'localhost' && body.origin === 'http://localhost:3000') {
        const id = randomBytes(16).toString('base64url');
        const challenge = randomBytes(32).toString('base64url');
        challenges.set(id, challenge);
        return Response.json({ challenge_id: id, challenge, rp_id: body.rp_id, allowed_credential_ids: [] });
      }
      if (url.pathname === '/v1/client/sign-in/passkey/verify' && body.session_mode === 'server') {
        const challenge = challenges.get(body.challenge_id);
        const data = JSON.parse(Buffer.from(body.client_data_json, 'base64url').toString());
        if (challenge && data.challenge === challenge && data.origin === 'http://localhost:3000' &&
            data.type === 'webauthn.get' && body.credential_id && body.signature && body.authenticator_data) {
          challenges.delete(body.challenge_id);
          return Response.json({ ticket });
        }
      }
    }
    return Response.json({ code: 'auth_refused' }, { status: 401 });
  }
  return new Response(null, { status: 404 });
} });
