import { expect, test } from 'bun:test';
import { createHash } from 'node:crypto';
import { redeemGrant, sealGrant } from '../apps/web/src/lib/cli-grant';

test('the CLI grant is opaque, verifier-bound, scoped to its tenant and expires', () => {
  const verifier = 'ab'.repeat(32);
  const challenge = createHash('sha256').update(verifier).digest('hex');
  const fixture = 'fixture-session';
  const grant = sealGrant('fixture-key', 'tenant-a', challenge, 'usr_fixture123', fixture, 1000);
  expect(grant).not.toContain(fixture);
  expect(redeemGrant('fixture-key', 'tenant-a', grant, verifier, 1500)?.token).toBe(fixture);
  expect(redeemGrant('fixture-key', 'tenant-b', grant, verifier, 1500)).toBeNull();
  expect(redeemGrant('fixture-key', 'tenant-a', grant, 'cd'.repeat(32), 1500)).toBeNull();
  expect(redeemGrant('fixture-key', 'tenant-a', grant, verifier, 61_000)).toBeNull();
  const corrupted = (grant[0] === 'A' ? 'B' : 'A') + grant.slice(1);
  expect(redeemGrant('fixture-key', 'tenant-a', corrupted, verifier, 1500)).toBeNull();
});
