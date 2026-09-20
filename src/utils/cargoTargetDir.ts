import { execFileSync } from 'node:child_process';
import path from 'node:path';

/**
 * The directory cargo actually builds into.
 *
 * Never guess `target/`: `CARGO_TARGET_DIR` moves it, and some setups append a
 * content-addressed subdirectory, so a hardcoded path points at a directory
 * cargo never writes and a just-built binary looks missing.
 *
 * `cargo metadata` reports the resolved `target_directory`, which is correct
 * for every configuration. Ask it once, then fall back to `$CARGO_TARGET_DIR`
 * and `<root>/target` when cargo is unavailable.
 */
const cache = new Map<string, string>();

export const resolveCargoTargetDir = (root: string): string => {
  const key = path.resolve(root);
  const hit = cache.get(key);
  if (hit !== undefined) return hit;

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

/** Profile directory (e.g. `release`, `debug`) inside cargo's target dir. */
export const resolveCargoProfileDir = (root: string, profile: 'release' | 'debug'): string =>
  path.join(resolveCargoTargetDir(root), profile);

/**
 * Every path a built Rust binary could be at, in priority order: the resolved
 * release dir, the resolved debug dir, then the literal `target/` paths so a
 * pre-existing checkout layout keeps working.
 */
export const cargoBinaryCandidates = (
  root: string,
  name: string,
  platformId?: string | null
): string[] => {
  const names = process.platform === 'win32' ? [`${name}.exe`, name] : [name, `${name}.exe`];
  const out: string[] = [];
  for (const candidateName of names) {
    out.push(path.join(resolveCargoProfileDir(root, 'release'), candidateName));
    out.push(path.join(resolveCargoProfileDir(root, 'debug'), candidateName));
  }
  for (const candidateName of names) {
    out.push(path.join(root, 'target/release', candidateName));
    out.push(path.join(root, 'target/debug', candidateName));
  }
  void platformId;
  return out;
};
