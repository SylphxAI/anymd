import { serve } from 'bun';
import { randomBytes } from 'node:crypto';

const ticket = randomBytes(24).toString('base64url');
const token = randomBytes(24).toString('base64url');
const flow = randomBytes(32).toString('base64url');
const challenges = new Map<string, string>();
serve({ hostname: '127.0.0.1', port: 3031, async fetch(request: Request) {
  const url = new URL(request.url);
  if (url.pathname === '/health') return new Response('ok');
  if (url.pathname === '/v1/client/config' && url.searchParams.get('publishable_key') === 'pk_fixture' && request.headers.get('authorization') === 'Bearer pk_fixture') {
    return Response.json(request.headers.get('user-agent') === 'anymd-test-otp-passkey' ? {
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
    if (request.headers.get('user-agent') === 'anymd-test-otp-passkey') {
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
    if (url.pathname === '/v1/client/sign-in/password' && body.email === 'test@example.invalid' &&
        body.password === 'fixture-long-password' && body.session_mode === 'server') return Response.json({ ticket });
    return Response.json({ code: 'auth_refused' }, { status: 401 });
  }
  return new Response(null, { status: 404 });
} });
