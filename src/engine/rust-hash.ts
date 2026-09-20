import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cargoBinaryCandidates } from '../utils/cargoTargetDir.js';

export type RustFileHash = {
  path: string;
  sourceHash: string;
  fileSize: number;
  route: string;
};

type RustHashEnvelope =
  | { status: 'ok'; hash: RustFileHash }
  | { status: 'error'; code: string; message: string };

const here = path.dirname(fileURLToPath(import.meta.url));

export function resolveRustCliBinary(): string {
  const env = process.env['PDF_READER_CLI'];
  if (env && existsSync(env)) {
    return env;
  }

  for (const candidate of cargoBinaryCandidates(path.join(here, '../..'), 'pdf-reader-cli')) {
    if (existsSync(candidate)) {
      return candidate;
    }
  }

  return 'pdf-reader-cli';
}

export function isRustCliAvailable(): boolean {
  return resolveRustCliBinary() !== 'pdf-reader-cli';
}

export function shouldUseRustHashEngine(): boolean {
  if (process.env['PDF_READER_USE_RUST_HASH'] === '0') {
    return false;
  }
  if (process.env['PDF_READER_USE_RUST_HASH'] === '1') {
    return true;
  }
  return isRustCliAvailable();
}

export function hashLocalFileViaRustEngine(
  filePath: string,
  maxFileBytes = 256 * 1024 * 1024
): string | undefined {
  const binary = resolveRustCliBinary();
  const payload = JSON.stringify({
    tool: 'pdf_hash',
    input: {
      path: filePath,
      max_file_bytes: maxFileBytes,
    },
  });

  const result = spawnSync(binary, [], {
    input: payload,
    encoding: 'utf8',
    maxBuffer: 4 * 1024 * 1024,
  });

  if (result.error || result.status !== 0) {
    return undefined;
  }

  const envelope = JSON.parse(result.stdout) as RustHashEnvelope;
  if (envelope.status !== 'ok') {
    return undefined;
  }

  return envelope.hash.sourceHash;
}
