#!/usr/bin/env bun

import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalReadOcrResult, type Json } from './v3014-read-ocr-residual-projection.ts';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-read-ocr-residual-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-read-ocr-residual-oracle.json');
const runnerPath = join(scriptDir, 'v3014-read-ocr-residual-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-read-ocr-residual-projection.ts');
const sharedProjectionPath = join(scriptDir, 'v3014-read-ocr-projection.ts');
const providerPath = join(scriptDir, 'reference-ocr-residual-provider.ts');
const visualFixturePath = join(fixtureDir, 'v3014-visual-v1.pdf');
const candidateFixturePath = join(fixtureDir, 'v3014-visual-candidate-v1.pdf');
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
  envelope: {
    fixtureCount: number;
    caseCount: number;
    maxPagesPerCase: number;
    maxOcrPagesPerCase: number;
  };
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
    sharedProjectionSha256: string;
    corpusSha256: string;
    providerSha256: string;
    fixtureSha256: Record<string, string>;
    pathPortabilityProof: boolean;
    envelope: Corpus['envelope'];
    nonclaims: Record<string, boolean>;
    entrypointSha256: Record<string, string>;
  };
  expectations: Record<string, Json>;
};

const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as Corpus;
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const baseline = oracle.baseline;
const commit = git('rev-list', '-n', '1', baseline.tag).toString().trim();
if (commit !== baseline.commit || commit !== '92651c79c6ce8d10dfa3c76332176c26f222bd78') {
  throw new Error('v3.0.14 read-ocr residual tag moved');
}
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== baseline.tree) {
  throw new Error('v3.0.14 read-ocr residual tree mismatch');
}
for (const [name, path, expected] of [
  ['runner', runnerPath, baseline.runnerSha256],
  ['projection', projectionPath, baseline.projectionSha256],
  ['sharedProjection', sharedProjectionPath, baseline.sharedProjectionSha256],
  ['corpus', corpusPath, baseline.corpusSha256],
  ['provider', providerPath, baseline.providerSha256],
  ['visualFixture', visualFixturePath, baseline.fixtureSha256['v3014-visual-v1.pdf']!],
  [
    'candidateFixture',
    candidateFixturePath,
    baseline.fixtureSha256['v3014-visual-candidate-v1.pdf']!,
  ],
] as const) {
  if (sha256(readFileSync(path)) !== expected) throw new Error(`read-ocr residual ${name} digest drift`);
}
for (const [path, expected] of Object.entries(baseline.entrypointSha256)) {
  if (sha256(git('show', `${commit}:${path}`)) !== expected) {
    throw new Error(`read-ocr residual entrypoint digest drift: ${path}`);
  }
}
if (sha256(git('show', `${commit}:bun.lock`)) !== baseline.bunLockSha256) {
  throw new Error('read-ocr residual bun.lock digest drift');
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
      entry.providerMode ?? 'plain-text',
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
        reject(new Error(`Rust residual read-ocr request timed out: ${stderr.slice(-2000)}`));
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
      clientInfo: { name: 'v3014-read-ocr-residual-rust', version: '1' },
    });
    child.stdin.write(
      `${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`
    );
    const input = structuredClone(entry.input);
    const sources = input.sources as Array<Record<string, unknown>>;
    for (const source of sources) source.path = join(root, entry.fixture);
    return canonicalReadOcrResult(
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

const relocatedDir = mkdtempSync(join(tmpdir(), 'pdf-reader-read-ocr-residual-relocated-'));
try {
  copyFileSync(visualFixturePath, join(relocatedDir, 'v3014-visual-v1.pdf'));
  copyFileSync(candidateFixturePath, join(relocatedDir, 'v3014-visual-candidate-v1.pdf'));
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
      throw new Error(`read-ocr residual comparator missed ${id}.${path.join('.')}`);
    }
    leafMutationCount += 1;
  }
}

const plain = oracle.expectations['ocr-plain-text-stdout'] as Record<string, Json>;
const jsonOnly = oracle.expectations['ocr-json-text-only-no-words'] as Record<string, Json>;
const firstFive = oracle.expectations['ocr-first-five-of-six'] as Record<string, Json>;
const plainData = ((plain.results as Json[])[0] as Record<string, Json>).data as Record<string, Json>;
const jsonData = ((jsonOnly.results as Json[])[0] as Record<string, Json>).data as Record<
  string,
  Json
>;
const firstData = ((firstFive.results as Json[])[0] as Record<string, Json>).data as Record<
  string,
  Json
>;
if (plainData.has_ocr_text_layer !== true) throw new Error('plain-text must include ocr_text_layer');
const plainLayer = plainData.ocr_text_layer as Record<string, Json>;
const plainPage = ((plainLayer.pages as Json[])[0] as Record<string, Json>) ?? {};
if (Object.hasOwn(plainPage, 'words')) throw new Error('plain-text fallback must omit words');
if (jsonData.has_ocr_text_layer !== true) throw new Error('json-text-only must include ocr_text_layer');
const jsonLayer = jsonData.ocr_text_layer as Record<string, Json>;
const jsonPage = ((jsonLayer.pages as Json[])[0] as Record<string, Json>) ?? {};
if (Object.hasOwn(jsonPage, 'words')) throw new Error('json-text-only must omit words');
if (firstData.has_ocr_text_layer !== true) throw new Error('first-five-of-six must include layer');
const warnings = (firstData.warnings as string[]) ?? [];
if (
  !warnings.some((warning) =>
    warning.includes('Rendered first 5 selected pages; skipped 6 due to max_pages.')
  )
) {
  throw new Error('first-five-of-six must emit max_pages truncation warning');
}
const firstLayer = firstData.ocr_text_layer as Record<string, Json>;
const firstPages = (firstLayer.pages as Json[]) ?? [];
if (firstPages.length !== 5) {
  throw new Error(`first-five-of-six must retain exactly 5 OCR pages, got ${firstPages.length}`);
}
const map = firstData.document_map as Record<string, Json>;
if (map.ocr_page_count !== 5) {
  throw new Error('document_map ocr_page_count must equal 5 for first-five-of-six');
}

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
}

const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_read_ocr_residual_result',
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
    plainTextStdoutOmitsWords: true,
    jsonTextOnlyOmitsWords: true,
    firstFiveOfSixTruncates: true,
  },
  nonclaims: corpus.nonclaims,
  productTruth: { dropInFor3014: false, publishFreeze: true },
  capabilityStatus: { includeOcrTextLayer: 'PARTIAL' },
  pass: failures.length === 0,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
console.error(
  `v3.0.14 read-ocr residual differential: PASS (${String(corpus.cases.length)}/${String(corpus.cases.length)}, zero skipped)`
);
