import { serve } from 'bun';
serve({ hostname: '127.0.0.1', port: 3031, fetch(request: Request) {
  const url = new URL(request.url);
  if (url.pathname === '/health') return new Response('ok');
  if (url.pathname === '/v1/client/config' && url.searchParams.get('publishable_key') === 'fixture') {
    return Response.json({ social: [{ provider: 'google', credentials: 'shared' }] });
  }
  return new Response(null, { status: 404 });
} });
