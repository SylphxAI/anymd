#!/usr/bin/env bun

import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';
import {
  canonicalAnnotationLinkAaPrecedenceResidualResult,
  type Json,
} from './v3014-annotation-link-aa-precedence-residual-projection.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-annotation-link-aa-precedence-residual-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-annotation-link-aa-precedence-residual-oracle.json');
const runnerPath = join(scriptDir, 'v3014-annotation-link-aa-precedence-residual-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-annotation-link-aa-precedence-residual-projection.ts');
const aaEFixture = join(fixtureDir, 'v3014-annotation-link-aa-e-ignored-v1.pdf');
const destAaFixture = join(fixtureDir, 'v3014-annotation-link-dest-over-aa-v1.pdf');
const aaDOverUFixture = join(fixtureDir, 'v3014-annotation-link-aa-d-over-u-v1.pdf');
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
  throw new Error('v3.0.14 annotation AP link-aa-precedence residual tag moved');
}
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== baseline.tree) {
  throw new Error('v3.0.14 annotation AP link-aa-precedence residual tree mismatch');
}
for (const [name, path, expected] of [
  ['runner', runnerPath, baseline.runnerSha256],
  ['projection', projectionPath, baseline.projectionSha256],
  ['corpus', corpusPath, baseline.corpusSha256],
  [
    'aaEFixture',
    aaEFixture,
    baseline.fixtureSha256['v3014-annotation-link-aa-e-ignored-v1.pdf']!,
  ],
  [
    'destAaFixture',
    destAaFixture,
    baseline.fixtureSha256['v3014-annotation-link-dest-over-aa-v1.pdf']!,
  ],
  [
    'aaDOverUFixture',
    aaDOverUFixture,
    baseline.fixtureSha256['v3014-annotation-link-aa-d-over-u-v1.pdf']!,
  ],
] as const) {
  if (sha256(readFileSync(path)) !== expected) {
    throw new Error(`annotation AP link-aa-precedence residual ${name} digest drift`);
  }
}
for (const [path, expected] of Object.entries(baseline.entrypointSha256)) {
  if (sha256(git('show', `${commit}:${path}`)) !== expected) {
    throw new Error(`annotation AP link-aa-precedence residual entrypoint digest drift: ${path}`);
  }
}
if (sha256(git('show', `${commit}:bun.lock`)) !== baseline.bunLockSha256) {
  throw new Error('annotation AP link-aa-precedence residual bun.lock digest drift');
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
  const request = (
    id: number,
    method: string,
    params: unknown
  ): Promise<Record<string, unknown>> =>
    new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`Rust annotation AP link-aa-precedence residual timed out: ${stderr.slice(-2000)}`));
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
      clientInfo: { name: 'v3014-annotation-link-aa-precedence-residual-rust', version: '1' },
    });
    child.stdin.write(
      `${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`
    );
    const input = structuredClone(entry.input);
    const sources = input.sources as Array<Record<string, unknown>>;
    for (const source of sources) source.path = join(root, entry.fixture);
    return canonicalAnnotationLinkAaPrecedenceResidualResult(
      await request(2, 'tools/call', { name: 'read_pdf', arguments: input })
    );
  } finally {
    child.kill('SIGTERM');
  }
};

const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
const observations: Record<string, Json> = {};
for (const entry of corpus.cases) {
  const actual = await invoke(entry, fixtureDir);
  observations[entry.id] = actual;
  const expected = oracle.expectations[entry.id]!;
  if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
}

const relocatedDir = mkdtempSync(join(tmpdir(), 'pdf-reader-annotation-ap-named-state-relocated-'));
try {
  copyFileSync(aaEFixture, join(relocatedDir, 'v3014-annotation-link-aa-e-ignored-v1.pdf'));
  copyFileSync(destAaFixture, join(relocatedDir, 'v3014-annotation-link-dest-over-aa-v1.pdf'));
  copyFileSync(aaDOverUFixture, join(relocatedDir, 'v3014-annotation-link-aa-d-over-u-v1.pdf'));
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
      throw new Error(`annotation AP link-aa-precedence residual comparator missed ${id}.${path.join('.')}`);
    }
    leafMutationCount += 1;
  }
}

const dataOf = (id: string): Record<string, Json> => {
  const result = oracle.expectations[id] as Record<string, Json>;
  return ((result.results as Json[])[0] as Record<string, Json>).data as Record<string, Json>;
};
const aaE = ((
  dataOf('link-aa-e-ignored').annotations as Json[]
)[0] as Record<string, Json>).annotations as Json[];
const destOverAa = ((
  dataOf('link-dest-over-aa').annotations as Json[]
)[0] as Record<string, Json>).annotations as Json[];
const aaDOverU = ((
  dataOf('link-aa-d-over-u').annotations as Json[]
)[0] as Record<string, Json>).annotations as Json[];
const assertBase = (ann: Record<string, Json>, label: string) => {
  if (ann.subtype !== 'Link' || ann.id !== '7R' || ann.page !== 1) {
    throw new Error(`${label}: unexpected base fields ${JSON.stringify(ann)}`);
  }
};
if (aaE.length !== 1) throw new Error('AA/E: expected 1 annotation');
assertBase(aaE[0] as Record<string, Json>, 'AA/E');
if (Object.hasOwn(aaE[0] as Record<string, Json>, 'url') || Object.hasOwn(aaE[0] as Record<string, Json>, 'dest')) {
  throw new Error(`AA/E must project neither url nor dest: ${JSON.stringify(aaE[0])}`);
}
if (destOverAa.length !== 1) throw new Error('Dest over AA: expected 1 annotation');
assertBase(destOverAa[0] as Record<string, Json>, 'Dest over AA');
if ((destOverAa[0] as Record<string, Json>).dest !== 'KeepDest') {
  throw new Error(`Dest over AA expected dest KeepDest: ${JSON.stringify(destOverAa[0])}`);
}
if (Object.hasOwn(destOverAa[0] as Record<string, Json>, 'url')) {
  throw new Error('Dest presence must suppress AA/U url');
}
if (aaDOverU.length !== 1) throw new Error('AA/D over U: expected 1 annotation');
assertBase(aaDOverU[0] as Record<string, Json>, 'AA/D over U');
if ((aaDOverU[0] as Record<string, Json>).dest !== 'FromD') {
  throw new Error(`AA/D over U expected dest FromD: ${JSON.stringify(aaDOverU[0])}`);
}
if (Object.hasOwn(aaDOverU[0] as Record<string, Json>, 'url')) {
  throw new Error('AA/D must win over AA/U url');
}

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
}

const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_annotation_link_aa_precedence_residual_result',
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
    linkAaEIgnored: true,
    linkDestOverAa: true,
    linkAaDOverU: true,
  },
  nonclaims: corpus.nonclaims,
  productTruth: { dropInFor3014: false, publishFreeze: true },
  capabilityStatus: { includeAnnotations: 'PARTIAL' },
  pass: failures.length === 0,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
console.error(
  `v3.0.14 annotation AP link-aa-precedence residual differential: PASS (${String(corpus.cases.length)}/${String(corpus.cases.length)}, zero skipped)`
);
