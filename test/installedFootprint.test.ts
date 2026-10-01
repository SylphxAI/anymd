import { describe, expect, test } from 'bun:test';
import { duBytes } from '../scripts/perf/du-bytes.js';

type DuResult = { status: number | null; stdout: string; stderr: string };
const ok = (stdout: string): DuResult => ({ status: 0, stdout, stderr: '' });
const failed: DuResult = { status: 1, stdout: '', stderr: 'du failed' };

function producer(...results: DuResult[]) {
  const calls: string[][] = [];
  const produce = (command: string, args: string[]) => {
    expect(command).toBe('du');
    calls.push(args);
    const result = results.shift();
    if (!result) throw new Error('unexpected du call');
    return result;
  };
  return { produce, calls };
}

describe('installed footprint producer status', () => {
  test('uses successful GNU byte counts without a fallback', () => {
    const { produce, calls } = producer(ok('12345\t/path with spaces\n'));
    expect(duBytes('/path with spaces', produce)).toBe(12345);
    expect(calls).toEqual([['-sb', '/path with spaces']]);
  });

  test('preserves successful portable KiB fallback and genuine zero', () => {
    const { produce, calls } = producer(failed, ok('12\t/path\n'));
    expect(duBytes('/path', produce)).toBe(12 * 1024);
    expect(calls).toEqual([
      ['-sb', '/path'],
      ['-sk', '/path'],
    ]);
    expect(duBytes('/empty', producer(ok('0\t/empty\n')).produce)).toBe(0);
  });

  test('rejects fallback failures even with numeric output', () => {
    for (const status of [1, null]) {
      const { produce } = producer(failed, { status, stdout: '42\t/path', stderr: 'failed' });
      expect(() => duBytes('/path', produce)).toThrow('du -sk failed');
    }
  });

  test('rejects no output, malformed, negative, nonfinite and unsafe counts on either path', () => {
    for (const output of [
      '',
      '   \n',
      'not-a-count\t/path',
      '-1\t/path',
      '1.5\t/path',
      'NaN\t/path',
      'Infinity\t/path',
      '1e309\t/path',
      '9007199254740992\t/path',
    ]) {
      expect(() => duBytes('/path', producer(ok(output)).produce)).toThrow('Invalid du byte count');
      expect(() => duBytes('/path', producer(failed, ok(output)).produce)).toThrow(
        'Invalid du byte count'
      );
    }
    expect(() => duBytes('/path', producer(failed, ok('9007199254740\t/path')).produce)).toThrow(
      'Invalid du byte count'
    );
  });
});
