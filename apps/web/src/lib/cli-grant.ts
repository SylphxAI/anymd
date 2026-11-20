import { createCipheriv, createDecipheriv, createHash, hkdfSync, randomBytes, timingSafeEqual } from 'node:crypto';

export const challengePattern = /^[a-f0-9]{64}$/;
export const statePattern = /^[a-f0-9]{32}$/;
const lifetime = 60_000;
const aad = 'anymd-cli-grant/v1';

type Grant = { challenge: string; subject: string; token: string; expires: number };

function key(secret: string, tenant: string) {
  return Buffer.from(hkdfSync('sha256', secret, tenant, aad, 32));
}

export function sealGrant(secret: string, tenant: string, challenge: string, subject: string, token: string, now = Date.now()) {
  const iv = randomBytes(12);
  const cipher = createCipheriv('aes-256-gcm', key(secret, tenant), iv);
  cipher.setAAD(Buffer.from(aad));
  const payload = JSON.stringify({ challenge, subject, token, expires: now + lifetime });
  return Buffer.concat([iv, cipher.update(payload), cipher.final(), cipher.getAuthTag()]).toString('base64url');
}

export function redeemGrant(secret: string, tenant: string, sealed: string, verifier: string, now = Date.now()): Grant | null {
  if (!challengePattern.test(verifier) || !/^[\w-]{40,8192}$/.test(sealed)) return null;
  try {
    const raw = Buffer.from(sealed, 'base64url');
    if (raw.toString('base64url') !== sealed) return null;
    const decipher = createDecipheriv('aes-256-gcm', key(secret, tenant), raw.subarray(0, 12));
    decipher.setAAD(Buffer.from(aad));
    decipher.setAuthTag(raw.subarray(-16));
    const data: Grant = JSON.parse(Buffer.concat([decipher.update(raw.subarray(12, -16)), decipher.final()]).toString('utf8'));
    const expected = createHash('sha256').update(verifier).digest();
    return data.expires > now && data.expires <= now + lifetime && challengePattern.test(data.challenge) &&
      timingSafeEqual(expected, Buffer.from(data.challenge, 'hex')) ? data : null;
  } catch { return null; }
}
