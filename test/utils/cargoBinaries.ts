import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

/** The product version every manifest carries (see scripts/set-version.ts). */
export const productVersion = (
  JSON.parse(readFileSync(path.join(repoRoot, 'packages/anymd/package.json'), 'utf8')) as {
    version: string;
  }
).version;

/** cargo's real target directory: `CARGO_TARGET_DIR` and cargo config can move it. */
const cargoTargetDir = (): string => {
  try {
    const output = execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], {
      cwd: repoRoot,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    return (JSON.parse(output) as { target_directory: string }).target_directory;
  } catch {
    return process.env['CARGO_TARGET_DIR'] || path.join(repoRoot, 'target');
  }
};

/**
 * The anymd binary under test: `ANYMD_BIN` when set, else the release build,
 * else the debug build. Falls back to the release path when nothing is built,
 * so "is it built?" assertions fail with a clear path.
 */
export const resolveServerPath = (): string => {
  const override = process.env['ANYMD_BIN'];
  if (override && existsSync(override)) return override;
  const exe = process.platform === 'win32' ? 'anymd.exe' : 'anymd';
  const target = cargoTargetDir();
  const candidates = [path.join(target, 'release', exe), path.join(target, 'debug', exe)];
  return candidates.find((candidate) => existsSync(candidate)) ?? (candidates[0] as string);
};
