import { spawnSync } from 'node:child_process';

type DuProducer = (
  command: string,
  args: string[],
  options: { encoding: 'utf8' }
) => { status: number | null; stdout: string; stderr: string };

function parseBytes(output: string, unit: number): number {
  const count = output.trim().split(/\s+/)[0] ?? '';
  const bytes = Number(count) * unit;
  if (!/^\d+$/.test(count) || !Number.isSafeInteger(bytes)) {
    throw new Error(`Invalid du byte count: ${JSON.stringify(output)}`);
  }
  return bytes;
}

export function duBytes(path: string, produce: DuProducer = spawnSync): number {
  const r = produce('du', ['-sb', path], { encoding: 'utf8' });
  if (r.status === 0) return parseBytes(r.stdout, 1);
  // macOS du lacks -sb; retain the portable KiB fallback, but require success.
  const fallback = produce('du', ['-sk', path], { encoding: 'utf8' });
  if (fallback.status !== 0) {
    throw new Error(`du -sk failed for ${path} (status ${fallback.status}): ${fallback.stderr}`);
  }
  return parseBytes(fallback.stdout, 1024);
}
