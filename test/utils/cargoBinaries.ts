import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cargoBinaryCandidates } from '../../src/utils/cargoTargetDir.js';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

/**
 * The built Rust binaries under test.
 *
 * Resolved through cargo's real target directory so a `CARGO_TARGET_DIR` (or a
 * content-addressed target dir) does not make a just-built binary look missing.
 * Falls back to the literal `target/` path when nothing else exists, which is
 * what keeps the "is it built?" assertions meaningful.
 */
export const resolveCliPath = (): string => {
  for (const candidate of cargoBinaryCandidates(repoRoot, 'pdf-reader-cli')) {
    if (existsSync(candidate)) return candidate;
  }
  return path.join(repoRoot, 'target/release/pdf-reader-cli');
};

export const resolveServerPath = (): string => {
  for (const candidate of cargoBinaryCandidates(repoRoot, 'citra-mcp-server')) {
    if (existsSync(candidate)) return candidate;
  }
  return path.join(repoRoot, 'target/release/citra-mcp-server');
};
