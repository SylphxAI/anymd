#!/usr/bin/env bun

import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';
import {
  canonicalPolylinePolygonResidualResult,
  type Json,
} from './v3014-polyline-polygon-residual-projection.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-polyline-polygon-residual-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-polyline-polygon-residual-oracle.json');
const runnerPath = join(scriptDir, 'v3014-polyline-polygon-residual-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-polyline-polygon-residual-projection.ts');
const polylineFixture = join(fixtureDir, 'v3014-annotation-polyline-l-bbox-v1.pdf');
const polygonFixture = join(fixtureDir, 'v3014-annotation-polygon-l-bbox-v1.pdf');
const border2Fixture = join(fixtureDir, 'v3014-annotation-polyline-border2-v1.pdf');
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
  throw new Error('v3.0.14 polyline/polygon residual tag moved');
}
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== baseline.tree) {
  throw new Error('v3.0.14 polyline/polygon residual tree mismatch');
}
for (const [name, path, expected] of [
  ['runner', runnerPath, baseline.runnerSha256],
  ['projection', projectionPath, baseline.projectionSha256],
  ['corpus', corpusPath, baseline.corpusSha256],
  [
    'polylineFixture',
    polylineFixture,
    baseline.fixtureSha256['v3014-annotation-polyline-l-bbox-v1.pdf']!,
  ],
  [
    'polygonFixture',
    polygonFixture,
    baseline.fixtureSha256['v3014-annotation-polygon-l-bbox-v1.pdf']!,
  ],
  [
    'border2Fixture',
    border2Fixture,
    baseline.fixtureSha256['v3014-annotation-polyline-border2-v1.pdf']!,
  ],
] as const) {
  if (sha256(readFileSync(path)) !== expected) {
    throw new Error(`polyline/polygon residual ${name} digest drift`);
  }
}
for (const [path, expected] of Object.entries(baseline.entrypointSha256)) {
  if (sha256(git('show', `${commit}:${path}`)) !== expected) {
    throw new Error(`polyline/polygon residual entrypoint digest drift: ${path}`);
  }
}
if (sha256(git('show', `${commit}:bun.lock`)) !== baseline.bunLockSha256) {
  throw new Error('polyline/polygon residual bun.lock digest drift');
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
        reject(new Error(`Rust polyline/polygon residual timed out: ${stderr.slice(-2000)}`));
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
      clientInfo: { name: 'v3014-polyline-polygon-residual-rust', version: '1' },
    });
    child.stdin.write(
      `${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`
    );
    const input = structuredClone(entry.input);
    const sources = input.sources as Array<Record<string, unknown>>;
    for (const source of sources) source.path = join(root, entry.fixture);
    return canonicalPolylinePolygonResidualResult(
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

const relocatedDir = mkdtempSync(join(tmpdir(), 'pdf-reader-polyline-polygon-relocated-'));
try {
  copyFileSync(polylineFixture, join(relocatedDir, 'v3014-annotation-polyline-l-bbox-v1.pdf'));
  copyFileSync(polygonFixture, join(relocatedDir, 'v3014-annotation-polygon-l-bbox-v1.pdf'));
  copyFileSync(border2Fixture, join(relocatedDir, 'v3014-annotation-polyline-border2-v1.pdf'));
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
      throw new Error(`polyline/polygon residual comparator missed ${id}.${path.join('.')}`);
    }
    leafMutationCount += 1;
  }
}

const dataOf = (id: string): Record<string, Json> => {
  const result = oracle.expectations[id] as Record<string, Json>;
  return ((result.results as Json[])[0] as Record<string, Json>).data as Record<string, Json>;
};
const polyline = ((
  dataOf('polyline-nonintersecting-rect-uses-vertices-bbox').annotations as Json[]
)[0] as Record<string, Json>).annotations as Json[];
const polygon = ((
  dataOf('polygon-nonintersecting-rect-uses-vertices-bbox').annotations as Json[]
)[0] as Record<string, Json>).annotations as Json[];
const border2 = ((
  dataOf('polyline-border-width-2-vertices-bbox').annotations as Json[]
)[0] as Record<string, Json>).annotations as Json[];
const polylineBox = (polyline[0] as Record<string, Json>).bounding_box as Record<string, Json>;
const polygonBox = (polygon[0] as Record<string, Json>).bounding_box as Record<string, Json>;
const border2Box = (border2[0] as Record<string, Json>).bounding_box as Record<string, Json>;
if (
  polylineBox.left !== 98 ||
  polylineBox.bottom !== 98 ||
  polylineBox.right !== 202 ||
  polylineBox.top !== 202
) {
  throw new Error('non-intersecting PolyLine must adopt vertices bbox expanded by 2*borderWidth');
}
if (
  polygonBox.left !== 48 ||
  polygonBox.bottom !== 48 ||
  polygonBox.right !== 122 ||
  polygonBox.top !== 122
) {
  throw new Error('non-intersecting Polygon must adopt vertices bbox expanded by 2*borderWidth');
}
if (
  border2Box.left !== 6 ||
  border2Box.bottom !== 6 ||
  border2Box.right !== 104 ||
  border2Box.top !== 84
) {
  throw new Error('BS W=2 PolyLine must expand vertices bbox by 4');
}
if ((polyline[0] as Record<string, Json>).subtype !== 'PolyLine') {
  throw new Error('polyline residual subtype must remain PolyLine');
}
if ((polygon[0] as Record<string, Json>).subtype !== 'Polygon') {
  throw new Error('polygon residual subtype must remain Polygon');
}

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
}

const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_polyline_polygon_residual_result',
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
    polylineNonIntersectingUsesVerticesBbox: true,
    polygonNonIntersectingUsesVerticesBbox: true,
    borderWidth2VerticesBbox: true,
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
  `v3.0.14 polyline/polygon residual differential: PASS (${String(corpus.cases.length)}/${String(corpus.cases.length)}, zero skipped)`
);
