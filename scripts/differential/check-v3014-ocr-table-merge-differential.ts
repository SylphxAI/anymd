#!/usr/bin/env bun

import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalOcrTableMergeResult, type Json } from './v3014-ocr-table-merge-projection.ts';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-ocr-table-merge-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-ocr-table-merge-oracle.json');
const runnerPath = join(scriptDir, 'v3014-ocr-table-merge-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-ocr-table-merge-projection.ts');
const providerPath = join(scriptDir, 'reference-ocr-table-merge-provider.ts');
const selectableFixture = join(fixtureDir, 'v3014-selectable-table-v1.pdf');
const visualFixture = join(fixtureDir, 'v3014-visual-v1.pdf');
const serverPath = join(resolveCargoReleaseDir(repoRoot), 'anymd');
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

type Case = {
  id: string;
  fixture: string;
  providerMode?: string;
  input: Record<string, unknown>;
};
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
    providerSha256: string;
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
  throw new Error('v3.0.14 ocr-table-merge tag moved');
}
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== baseline.tree) {
  throw new Error('v3.0.14 ocr-table-merge tree mismatch');
}
for (const [name, path, expected] of [
  ['runner', runnerPath, baseline.runnerSha256],
  ['projection', projectionPath, baseline.projectionSha256],
  ['corpus', corpusPath, baseline.corpusSha256],
  ['provider', providerPath, baseline.providerSha256],
  ['selectableFixture', selectableFixture, baseline.fixtureSha256['v3014-selectable-table-v1.pdf']!],
  ['visualFixture', visualFixture, baseline.fixtureSha256['v3014-visual-v1.pdf']!],
] as const) {
  if (sha256(readFileSync(path)) !== expected) throw new Error(`ocr-table-merge ${name} digest drift`);
}
for (const [path, expected] of Object.entries(baseline.entrypointSha256)) {
  if (sha256(git('show', `${commit}:${path}`)) !== expected) {
    throw new Error(`ocr-table-merge entrypoint digest drift: ${path}`);
  }
}
if (sha256(git('show', `${commit}:bun.lock`)) !== baseline.bunLockSha256) {
  throw new Error('ocr-table-merge bun.lock digest drift');
}
if (!existsSync(serverPath)) {
  const build = spawnSync('cargo', ['build', '-p', 'pdf-reader-mcp-server', '--release'], {
    cwd: repoRoot,
    encoding: 'utf8',
  });
  if (build.status !== 0) throw new Error(build.stderr || 'release server build failed');
}

const invoke = async (entry: Case, root: string): Promise<Json> => {
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    MCP_TRANSPORT: 'stdio',
    MCP_PDF_OCR_COMMAND: process.execPath,
    MCP_PDF_OCR_ARGS_JSON: JSON.stringify([
      providerPath,
      '{input}',
      '{page}',
      '{languages}',
      entry.providerMode ?? 'distinct',
    ]),
  };
  delete env.MCP_PDF_OCR_PRESET;
  const child = spawn(serverPath, [], {
    cwd: repoRoot,
    stdio: ['pipe', 'pipe', 'pipe'],
    env,
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
        reject(new Error(`Rust ocr-table-merge request timed out: ${stderr.slice(-2000)}`));
      }, 180_000);
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
      clientInfo: { name: 'v3014-ocr-table-merge-rust', version: '1' },
    });
    child.stdin.write(
      `${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`
    );
    const input = structuredClone(entry.input);
    const sources = input.sources as Array<Record<string, unknown>>;
    for (const source of sources) source.path = join(root, entry.fixture);
    return canonicalOcrTableMergeResult(
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

const relocatedDir = mkdtempSync(join(tmpdir(), 'pdf-reader-ocr-table-merge-relocated-'));
try {
  copyFileSync(selectableFixture, join(relocatedDir, 'v3014-selectable-table-v1.pdf'));
  copyFileSync(visualFixture, join(relocatedDir, 'v3014-visual-v1.pdf'));
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
      throw new Error(`ocr-table-merge comparator missed ${id}.${path.join('.')}`);
    }
    leafMutationCount += 1;
  }
}

const distinct = oracle.expectations['ocr-table-merge-distinct'] as Record<string, Json>;
const overlap = oracle.expectations['ocr-table-overlap-suppress'] as Record<string, Json>;
const only = oracle.expectations['ocr-table-only-page3'] as Record<string, Json>;
const distinctData = ((distinct.results as Json[])[0] as Record<string, Json>).data as Record<
  string,
  Json
>;
const overlapData = ((overlap.results as Json[])[0] as Record<string, Json>).data as Record<
  string,
  Json
>;
const onlyData = ((only.results as Json[])[0] as Record<string, Json>).data as Record<string, Json>;
if (distinctData.table_count !== 2) throw new Error('distinct merge must yield 2 tables');
if (
  JSON.stringify(distinctData.table_provenance_sources) !==
  JSON.stringify(['selectable_text', 'ocr_text_layer'])
) {
  throw new Error('distinct merge must keep selectable then ocr provenance order');
}
if (overlapData.table_count !== 1) throw new Error('overlap must suppress OCR duplicate');
if (JSON.stringify(overlapData.table_provenance_sources) !== JSON.stringify(['selectable_text'])) {
  throw new Error('overlap must retain only selectable_text provenance');
}
if (onlyData.table_count !== 1) throw new Error('page3-only must yield one OCR table');
if (JSON.stringify(onlyData.table_provenance_sources) !== JSON.stringify(['ocr_text_layer'])) {
  throw new Error('page3-only must be ocr_text_layer provenance');
}

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
}

const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_ocr_table_merge_result',
  candidateSha,
  baselineCommit: commit,
  baselineTree: baseline.tree,
  corpusSha256: sha256(readFileSync(corpusPath)),
  oracleSha256: sha256(readFileSync(oraclePath)),
  runnerSha256: sha256(readFileSync(runnerPath)),
  projectionSha256: sha256(readFileSync(projectionPath)),
  providerSha256: sha256(readFileSync(providerPath)),
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
    configuredCommandOnly: true,
    distinctMergeKeepsBoth: true,
    overlapSuppressesOcrDuplicate: true,
    ocrOnlyPageAdmitted: true,
  },
  nonclaims: corpus.nonclaims,
  productTruth: { dropInFor3014: false, publishFreeze: true },
  capabilityStatus: { includeTables: 'PARTIAL', includeOcrTextLayer: 'PARTIAL' },
  pass: failures.length === 0,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
console.error(
  `v3.0.14 ocr-table-merge differential: PASS (${String(corpus.cases.length)}/${String(corpus.cases.length)}, zero skipped)`
);
