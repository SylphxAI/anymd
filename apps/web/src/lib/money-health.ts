import { productionMoney } from './money-production.generated';
import { testMoneyOrigin, testMoneyParent } from './money-target';

const PROBE_TIMEOUT_MS = 2_000;
const SUCCESS_TTL_MS = 60_000;
const ERROR_TTL_MS = 15_000;

type ProbeResult = 'ok' | 'error';
type Health = ProbeResult | 'unconfigured';
type Target = { key: string; origin: string; parent: string };
let cache: (Target & { result: Promise<ProbeResult>; expiresAt: number }) | undefined;

function configuredTarget(): Target | undefined {
  // Unlike checkout, this diagnostic only observes the server binding; it
  // must not depend on the public hostname or a signed-in user.
  if (process.env.ANYMD_MONEY_MODE === 'live') {
    const key = process.env.SYLPHX_MONEY_API_KEY?.trim();
    if (key?.startsWith('sylphx_sk_live_')) return { key, origin: productionMoney.origin, parent: productionMoney.parent };
    return;
  }
  const key = process.env.ANYMD_MONEY_TEST_API_KEY?.trim();
  const parent = testMoneyParent();
  if (!key?.startsWith('sylphx_sk_test_') || !parent) return;
  try {
    const origin = testMoneyOrigin();
    if (origin) return { key, origin, parent };
  } catch {
    // A malformed preview URL cannot be used to send credentials.
  }
}

async function probe({ key, origin, parent }: Target): Promise<ProbeResult> {
  try {
    // The same fetch signal covers the response headers and body consumption.
    const response = await fetch(new URL('/v1/whoami', origin), {
      method: 'GET',
      headers: { Authorization: 'Bearer ' + key, Accept: 'application/json' },
      cache: 'no-store',
      redirect: 'error',
      signal: AbortSignal.timeout(PROBE_TIMEOUT_MS),
    });
    if (!response.ok) {
      void response.body?.cancel().catch(() => {});
      return 'error';
    }
    const body: { env?: unknown } | null = await response.json();
    const env = typeof body?.env === 'string' && body.env.replace(/^\/+|\/+$/g, '');
    return env && env === parent ? 'ok' : 'error';
  } catch {
    return 'error';
  }
}

export async function moneyHealth(): Promise<Health> {
  const target = configuredTarget();
  if (!target) {
    cache = undefined;
    return 'unconfigured';
  }
  if (cache?.key === target.key && cache.origin === target.origin &&
      cache.parent === target.parent && performance.now() < cache.expiresAt) return cache.result;

  const result = probe(target).then(outcome => {
    // A probe of a superseded binding must not overwrite the new result.
    if (cache?.result === result) {
      cache.expiresAt = performance.now() + (outcome === 'ok' ? SUCCESS_TTL_MS : ERROR_TTL_MS);
    }
    return outcome;
  });
  // No expiry while a bounded refresh is in flight; set its TTL on completion.
  cache = { ...target, result, expiresAt: Infinity };
  return result;
}
