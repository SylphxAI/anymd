import { moneyHealth } from '../../lib/money-health';

export async function GET() {
  return Response.json({ status: 'ok', service: 'anymd-web', money: await moneyHealth() }, {
    headers: { 'Cache-Control': 'private, no-store' },
  });
}
