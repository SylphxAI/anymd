export const measurementId = 'G-R4TD2Z8RR7';
export const clickCookie = 'anymd_attribution';

const clickKeys = ['gclid', 'gbraid', 'wbraid'] as const;
type ClickKey = typeof clickKeys[number];

export function clickIds(read: (key: ClickKey) => unknown) {
  const result: Partial<Record<ClickKey, string>> = {};
  for (const key of clickKeys) {
    const value = read(key);
    if (typeof value === 'string' && /^[A-Za-z0-9_.-]{1,200}$/.test(value)) result[key] = value;
  }
  return result;
}
