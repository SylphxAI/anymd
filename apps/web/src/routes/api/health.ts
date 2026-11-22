export function GET() {
  return Response.json({ status: 'ok', service: 'anymd-web' }, { headers: { 'Cache-Control': 'private, no-store' } });
}
