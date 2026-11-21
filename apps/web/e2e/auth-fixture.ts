import { serve } from 'bun';

const ticket = crypto.randomUUID();
const token = process.env.ANYMD_E2E_SESSION;
if (!token) throw new Error('Missing local test session');
serve({ hostname: '127.0.0.1', port: 3031, async fetch(request: Request) {
  const url = new URL(request.url);
  if (url.pathname === '/health') return new Response('ok');
  if (url.pathname === '/v1/client/config' && url.searchParams.get('publishable_key') === 'fixture') {
    return Response.json(request.headers.get('user-agent') === 'anymd-test-otp-passkey' ? {
      social: [], methods: { passkey: true, email_otp: true, password: false, sign_up: false },
    } : {
      social: [{ provider: 'google', credentials: 'shared' }],
      methods: { passkey: false, email_otp: false, password: true, sign_up: true },
    });
  }
  if (url.pathname === '/v1/sessions/current' && request.headers.get('authorization') === 'Bearer ' + token &&
      request.headers.get('x-sylphx-caller-key') === 'fixture') {
    return Response.json({ session: { principal: {
      project_id: '00000000-0000-0000-0000-000000000000', principal_id: 'usr_fixture001', primary_email: 'test@example.invalid', state: 'active',
    } } });
  }
  if (request.method === 'POST' && request.headers.get('authorization') === 'Bearer fixture') {
    const body = await request.json();
    if (url.pathname === '/v1/client/tickets:redeem' && body.ticket === ticket) return Response.json({ session: { token } });
    if (body.email === 'test@example.invalid' && body.password === 'fixture-long-password') {
      if (url.pathname === '/v1/client/sign-in/password' && body.session_mode === 'server') return Response.json({ ticket });
      if (url.pathname === '/v1/client/sign-up') return Response.json({ accepted: true }, { status: 202 });
    }
    return Response.json({ code: 'auth_refused' }, { status: 401 });
  }
  return new Response(null, { status: 404 });
} });
