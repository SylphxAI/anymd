import { execFileSync } from 'node:child_process';
import path from 'node:path';

/**
 * The directory cargo actually builds into.
 *
 * Never guess `target/`: `CARGO_TARGET_DIR` moves it, and some setups append a
 * content-addressed subdirectory, so a hardcoded path points at a directory
 * cargo never writes. Ask cargo instead — `cargo metadata` reports the resolved
 * `target_directory`, which is correct for every configuration.
 *
 * The first argument is the repository root the caller is working from, so a
 * script invoked from a worktree resolves that worktree's own target dir.
 * Falls back to `$CARGO_TARGET_DIR`, then `<root>/target`, only when cargo
 * cannot answer (no toolchain on PATH).
 */
const cache = new Map<string, string>();

export const resolveCargoTargetDir = (root: string = path.resolve(import.meta.dirname, '../..')): string => {
  const key = path.resolve(root);
  const hit = cache.get(key);
  if (hit) return hit;
  let resolved: string | null = null;
  try {
    const output = execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], {
      cwd: key,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    resolved = (JSON.parse(output) as { target_directory?: string }).target_directory ?? null;
  } catch {
    // cargo unavailable; fall through to the environment/default guess.
  }
  if (!resolved) {
    const envDir = process.env['CARGO_TARGET_DIR']?.trim();
    resolved = envDir ? path.resolve(envDir) : path.join(key, 'target');
  }
  cache.set(key, resolved);
  return resolved;
};

/** The release profile directory inside the resolved cargo target directory. */
export const resolveCargoReleaseDir = (
  root: string = path.resolve(import.meta.dirname, '../..')
): string => path.join(resolveCargoTargetDir(root), 'release');
