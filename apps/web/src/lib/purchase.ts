import { createHash } from 'node:crypto';
import type { Purchase } from './analytics';

// The app's privileged Money create uses a keyed, Money-assigned name.
export const checkoutIdPattern = /^chk-[0-9a-f]{24}$/;
export const processorSessionPattern = /^cs_(?:live|test)_[a-zA-Z0-9]{1,124}$/;

export function paidPurchase(session: Record<string, unknown>, expected: {
  name: string; processorSessionId: string; user: string; price: string; live: boolean; currencyScale: number;
}): Purchase | null {
  const line = Array.isArray(session.line_items) && session.line_items.length === 1 ? session.line_items[0] : null;
  const subject = session.subject as {end_user?: unknown} | undefined;
  if (session.name !== expected.name || session.processor_session_id !== expected.processorSessionId ||
      subject?.end_user !== expected.user || line?.price !== expected.price || line.quantity !== 1 ||
      session.state !== 'complete' || session.payment_status !== 'paid' || session.livemode !== expected.live ||
      typeof session.currency_code !== 'string' || !/^[A-Z]{3}$/.test(session.currency_code)) return null;
  const amount = typeof session.amount_total === 'string' && /^\d+$/.test(session.amount_total) ? Number(session.amount_total) : session.amount_total;
  if (typeof amount !== 'number' || !Number.isSafeInteger(amount) || amount <= 0) return null;
  // Stable GA transaction identifier for deduplication without disclosing the
  // raw provider receipt, Money resource name or user ID to Google.
  const transaction_id = createHash('sha256').update('anymd.ga4.purchase:' + expected.name).digest('hex');
  return {transaction_id, currency:session.currency_code, value:amount / expected.currencyScale};
}
