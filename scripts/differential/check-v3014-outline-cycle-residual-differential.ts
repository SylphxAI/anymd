#!/usr/bin/env bun

import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';
import {
  canonicalOutlineCycleResidualResult,
  type Json,
} from './v3014-outline-cycle-residual-projection.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-outline-cycle-residual-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-outline-cycle-residual-oracle.json');
const runnerPath = join(scriptDir, 'v3014-outline-cycle-residual-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-outline-cycle-residual-projection.ts');
const nextSelfFixture = join(fixtureDir, 'v3014-outline-cycle-next-self-v1.pdf');
const siblingFixture = join(fixtureDir, 'v3014-outline-cycle-sibling-v1.pdf');
const selfFirstFixture = join(fixtureDir, 'v3014-outline-cycle-self-first-v1.pdf');
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
  throw new Error('v3.0.14 outline cycle residual tag moved');
}
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== baseline.tree) {
  throw new Error('v3.0.14 outline cycle residual tree mismatch');
}
for (const [name, path, expected] of [
  ['runner', runnerPath, baseline.runnerSha256],
  ['projection', projectionPath, baseline.projectionSha256],
  ['corpus', corpusPath, baseline.corpusSha256],
  ['nextSelfFixture', nextSelfFixture, baseline.fixtureSha256['v3014-outline-cycle-next-self-v1.pdf']!],
  ['siblingFixture', siblingFixture, baseline.fixtureSha256['v3014-outline-cycle-sibling-v1.pdf']!],
  ['selfFirstFixture', selfFirstFixture, baseline.fixtureSha256['v3014-outline-cycle-self-first-v1.pdf']!],
] as const) {
  if (sha256(readFileSync(path)) !== expected) {
    throw new Error(`outline cycle residual ${name} digest drift`);
  }
}
for (const [path, expected] of Object.entries(baseline.entrypointSha256)) {
  if (sha256(git('show', `${commit}:${path}`)) !== expected) {
    throw new Error(`outline cycle residual entrypoint digest drift: ${path}`);
  }
}
if (sha256(git('show', `${commit}:bun.lock`)) !== baseline.bunLockSha256) {
  throw new Error('outline cycle residual bun.lock digest drift');
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
        reject(new Error(`Rust outline cycle residual timed out: ${stderr.slice(-2000)}`));
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
      clientInfo: { name: 'v3014-outline-cycle-residual-rust', version: '1' },
    });
    child.stdin.write(
      `${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`
    );
    const input = structuredClone(entry.input);
    const sources = input.sources as Array<Record<string, unknown>>;
    for (const source of sources) source.path = join(root, entry.fixture);
    return canonicalOutlineCycleResidualResult(
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

const relocatedDir = mkdtempSync(join(tmpdir(), 'pdf-reader-outline-relocated-'));
try {
  copyFileSync(nextSelfFixture, join(relocatedDir, 'v3014-outline-cycle-next-self-v1.pdf'));
  copyFileSync(siblingFixture, join(relocatedDir, 'v3014-outline-cycle-sibling-v1.pdf'));
  copyFileSync(selfFirstFixture, join(relocatedDir, 'v3014-outline-cycle-self-first-v1.pdf'));
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
      throw new Error(`outline cycle residual comparator missed ${id}.${path.join('.')}`);
    }
    leafMutationCount += 1;
  }
}

const dataOf = (id: string): Record<string, Json> => {
  const result = oracle.expectations[id] as Record<string, Json>;
  return ((result.results as Json[])[0] as Record<string, Json>).data as Record<string, Json>;
};
const nextSelf = dataOf('outline-cycle-next-self').outline as Array<Record<string, Json>>;
const sibling = dataOf('outline-cycle-sibling').outline as Array<Record<string, Json>>;
const selfFirst = dataOf('outline-cycle-self-first').outline as Array<Record<string, Json>>;
if (
  nextSelf.length !== 1 ||
  nextSelf[0]?.title !== 'Cycle' ||
  !Array.isArray(nextSelf[0]?.dest) ||
  (nextSelf[0]?.dest?.[1] as Record<string, Json>).name !== 'Fit'
) {
  throw new Error('Next self-cycle must project single Cycle item with Fit dest');
}
if (
  sibling.length !== 2 ||
  sibling[0]?.title !== 'Child' ||
  sibling[1]?.title !== 'Loop' ||
  !Array.isArray(sibling[0]?.dest) ||
  (sibling[0]?.dest?.[1] as Record<string, Json>).name !== 'Fit' ||
  !Array.isArray(sibling[1]?.dest) ||
  (sibling[1]?.dest?.[1] as Record<string, Json>).name !== 'FitH' ||
  sibling[1]?.dest?.[2] !== 700
) {
  throw new Error('Sibling Next cycle must project Child+Loop once each with Fit/FitH dests');
}
if (
  selfFirst.length !== 1 ||
  selfFirst[0]?.title !== 'Self' ||
  Object.hasOwn(selfFirst[0] ?? {}, 'items') ||
  !Array.isArray(selfFirst[0]?.dest) ||
  (selfFirst[0]?.dest?.[1] as Record<string, Json>).name !== 'Fit'
) {
  throw new Error('Self First cycle must project single Self item without nested items');
}

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
}

const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_outline_cycle_residual_result',
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
    nextSelfCycleSuppressed: true,
    siblingNextCycleSuppressed: true,
    selfFirstCycleSuppressed: true,
  },
  nonclaims: corpus.nonclaims,
  productTruth: { dropInFor3014: false, publishFreeze: true },
  capabilityStatus: { includeOutline: 'PARTIAL' },
  pass: failures.length === 0,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
console.error(
  `v3.0.14 outline cycle residual differential: PASS (${String(corpus.cases.length)}/${String(corpus.cases.length)}, zero skipped)`
);
