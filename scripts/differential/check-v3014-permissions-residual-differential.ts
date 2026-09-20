#!/usr/bin/env bun

import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';
import {
  canonicalPermissionsResidualResult,
  type Json,
} from './v3014-permissions-residual-projection.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-permissions-residual-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-permissions-residual-oracle.json');
const runnerPath = join(scriptDir, 'v3014-permissions-residual-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-permissions-residual-projection.ts');
const printCopyFixture = join(fixtureDir, 'v3014-permissions-print-copy-fill-a11y-v1.pdf');
const modifyFixture = join(fixtureDir, 'v3014-permissions-modify-annotate-assemble-v1.pdf');
const printHqFixture = join(fixtureDir, 'v3014-permissions-print-hq-v1.pdf');
const noneFixture = join(fixtureDir, 'v3014-permissions-none-v1.pdf');
const serverPath = join(resolveCargoReleaseDir(repoRoot), 'citra-mcp-server');
const outputIndex = process.argv.indexOf('--output');
const outputPath = outputIndex >= 0 ? process.argv[outputIndex + 1] : undefined;
const sha256 = (value: Uint8Array | string): string =>
  createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => {
  const result = spawnSync('git', args, { cwd: repoRoot });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};
const same = (left: Json, right: Json): boolean => JSON.stringify(left) === JSON.stringify(right);

type Case = { id: string; fixture: string; input: Record<string, unknown> };
type Corpus = {
  envelope: Record<string, number>;
  nonclaims: Record<string, boolean>;
  cases: Case[];
};
type Oracle = {
  baseline: {
    tag: string;
    commit: string;
    tree: string;
    bunLockSha256: string;
    runnerSha256: string;
    projectionSha256: string;
    corpusSha256: string;
    fixtureSha256: Record<string, string>;
    entrypointSha256: Record<string, string>;
  };
  expectations: Record<string, Json>;
};

const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as Corpus;
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const baseline = oracle.baseline;
const commit = git('rev-list', '-n', '1', baseline.tag).toString().trim();
if (commit !== baseline.commit || commit !== '92651c79c6ce8d10dfa3c76332176c26f222bd78') {
  throw new Error('v3.0.14 permissions residual tag moved');
}
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== baseline.tree) {
  throw new Error('v3.0.14 permissions residual tree mismatch');
}
for (const [name, path, expected] of [
  ['runner', runnerPath, baseline.runnerSha256],
  ['projection', projectionPath, baseline.projectionSha256],
  ['corpus', corpusPath, baseline.corpusSha256],
  [
    'printCopyFixture',
    printCopyFixture,
    baseline.fixtureSha256['v3014-permissions-print-copy-fill-a11y-v1.pdf']!,
  ],
  [
    'modifyFixture',
    modifyFixture,
    baseline.fixtureSha256['v3014-permissions-modify-annotate-assemble-v1.pdf']!,
  ],
  [
    'printHqFixture',
    printHqFixture,
    baseline.fixtureSha256['v3014-permissions-print-hq-v1.pdf']!,
  ],
  ['noneFixture', noneFixture, baseline.fixtureSha256['v3014-permissions-none-v1.pdf']!],
] as const) {
  if (sha256(readFileSync(path)) !== expected) {
    throw new Error(`permissions residual ${name} digest drift`);
  }
}
for (const [path, expected] of Object.entries(baseline.entrypointSha256)) {
  if (sha256(git('show', `${commit}:${path}`)) !== expected) {
    throw new Error(`permissions residual entrypoint digest drift: ${path}`);
  }
}
if (sha256(git('show', `${commit}:bun.lock`)) !== baseline.bunLockSha256) {
  throw new Error('permissions residual bun.lock digest drift');
}
if (!existsSync(serverPath)) {
  const build = spawnSync('cargo', ['build', '-p', 'pdf-reader-mcp-server', '--release'], {
    cwd: repoRoot,
    encoding: 'utf8',
  });
  if (build.status !== 0) throw new Error(build.stderr || 'release server build failed');
}

const invoke = async (entry: Case, root: string): Promise<Json> => {
  const child = spawn(serverPath, [], {
    cwd: repoRoot,
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, MCP_TRANSPORT: 'stdio' },
  }) as ChildProcessWithoutNullStreams;
  let buffer = '';
  let stderr = '';
  const pending = new Map<number, (value: Record<string, unknown>) => void>();
  child.stderr.on('data', (chunk: Buffer) => {
    stderr += chunk.toString();
  });
  child.stdout.on('data', (chunk: Buffer) => {
    buffer += chunk.toString();
    while (buffer.includes('\n')) {
      const index = buffer.indexOf('\n');
      const line = buffer.slice(0, index).trim();
      buffer = buffer.slice(index + 1);
      if (!line) continue;
      const response = JSON.parse(line) as Record<string, unknown>;
      pending.get(Number(response.id))?.(response);
      pending.delete(Number(response.id));
    }
  });
  const request = (id: number, method: string, params: unknown): Promise<Record<string, unknown>> =>
    new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`Rust permissions residual timed out: ${stderr.slice(-2000)}`));
      }, 120_000);
      pending.set(id, (value) => {
        clearTimeout(timer);
        resolve(value);
      });
      child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
    });
  try {
    await request(1, 'initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'v3014-permissions-residual-rust', version: '1' },
    });
    child.stdin.write(
      `${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`
    );
    const input = structuredClone(entry.input);
    const sources = input.sources as Array<Record<string, unknown>>;
    for (const source of sources) source.path = join(root, entry.fixture);
    return canonicalPermissionsResidualResult(
      await request(2, 'tools/call', { name: 'read_pdf', arguments: input })
    );
  } finally {
    child.kill('SIGTERM');
  }
};

const observations: Record<string, Json> = {};
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
for (const entry of corpus.cases) {
  const actual = await invoke(entry, fixtureDir);
  observations[entry.id] = actual;
  const expected = oracle.expectations[entry.id]!;
  if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
}

const relocatedDir = mkdtempSync(join(tmpdir(), 'pdf-reader-permissions-relocated-'));
try {
  copyFileSync(printCopyFixture, join(relocatedDir, 'v3014-permissions-print-copy-fill-a11y-v1.pdf'));
  copyFileSync(modifyFixture, join(relocatedDir, 'v3014-permissions-modify-annotate-assemble-v1.pdf'));
  copyFileSync(printHqFixture, join(relocatedDir, 'v3014-permissions-print-hq-v1.pdf'));
  copyFileSync(noneFixture, join(relocatedDir, 'v3014-permissions-none-v1.pdf'));
  for (const entry of corpus.cases) {
    const actual = await invoke(entry, relocatedDir);
    if (!same(actual, oracle.expectations[entry.id]!)) {
      throw new Error(`relocated fixture-root replay failed for ${entry.id}`);
    }
  }
} finally {
  rmSync(relocatedDir, { recursive: true, force: true });
}

const leaves = (value: Json, prefix: Array<string | number> = []): Array<Array<string | number>> =>
  Array.isArray(value)
    ? value.flatMap((entry, index) => leaves(entry, [...prefix, index]))
    : value && typeof value === 'object'
      ? Object.entries(value).flatMap(([key, entry]) => leaves(entry as Json, [...prefix, key]))
      : [prefix];
const mutate = (value: Json, path: Array<string | number>): Json => {
  const changed = structuredClone(value);
  let cursor = changed as never;
  for (const segment of path.slice(0, -1)) cursor = cursor[segment as never];
  const key = path.at(-1)!;
  const original = cursor[key as never] as Json;
  cursor[key as never] = (
    typeof original === 'string'
      ? `${original}-mutated`
      : typeof original === 'number'
        ? original + 1
        : original === null
          ? 'mutated'
          : !original
  ) as never;
  return changed;
};
let leafMutationCount = 0;
for (const [id, expectation] of Object.entries(oracle.expectations)) {
  for (const path of leaves(expectation)) {
    if (same(observations[id]!, mutate(expectation, path))) {
      throw new Error(`permissions residual comparator missed ${id}.${path.join('.')}`);
    }
    leafMutationCount += 1;
  }
}

const dataOf = (id: string): Record<string, Json> => {
  const result = oracle.expectations[id] as Record<string, Json>;
  return ((result.results as Json[])[0] as Record<string, Json>).data as Record<string, Json>;
};
const printCopy = dataOf('permissions-print-copy-fill-a11y');
const modify = dataOf('permissions-modify-annotate-assemble');
const printHq = dataOf('permissions-print-hq');
const absent = dataOf('permissions-absent-unencrypted');
if (
  JSON.stringify(printCopy.permissions) !==
  JSON.stringify(['print', 'copy', 'fill_forms', 'copy_for_accessibility'])
) {
  throw new Error('print/copy/fill/a11y labels mismatch');
}
if (
  JSON.stringify(modify.permissions) !== JSON.stringify(['modify', 'annotate', 'assemble'])
) {
  throw new Error('modify/annotate/assemble labels mismatch');
}
if (
  JSON.stringify(printHq.permissions) !== JSON.stringify(['print', 'print_high_quality'])
) {
  throw new Error('print+hq labels mismatch');
}
if (Object.hasOwn(absent, 'permissions')) {
  throw new Error('unencrypted PDF must omit permissions');
}

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
}

const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_permissions_residual_result',
  candidateSha,
  baselineCommit: commit,
  baselineTree: baseline.tree,
  corpusSha256: sha256(readFileSync(corpusPath)),
  oracleSha256: sha256(readFileSync(oraclePath)),
  runnerSha256: sha256(readFileSync(runnerPath)),
  projectionSha256: sha256(readFileSync(projectionPath)),
  fixtureSha256: baseline.fixtureSha256,
  entrypointSha256: baseline.entrypointSha256,
  envelope: corpus.envelope,
  caseCount: corpus.cases.length,
  passed: corpus.cases.length - failures.length,
  skipped: 0,
  mutationSensitive: { allClaimedFields: true, leafMutationCount },
  portabilityProof: {
    relocatedFixtureRootReplay: true,
    normalizedFixtureToken: '<fixture>',
  },
  providerProof: {
    printCopyFillA11y: true,
    modifyAnnotateAssemble: true,
    printHighQuality: true,
    unencryptedOmitsPermissions: true,
  },
  nonclaims: corpus.nonclaims,
  productTruth: { dropInFor3014: false, publishFreeze: true },
  capabilityStatus: { includePermissions: 'PARTIAL' },
  pass: failures.length === 0,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
console.error(
  `v3.0.14 permissions residual differential: PASS (${String(corpus.cases.length)}/${String(corpus.cases.length)}, zero skipped)`
);
