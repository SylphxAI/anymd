#!/usr/bin/env bun

import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PNG } from 'pngjs';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';
import {
  canonicalRasterImageResult,
  RASTER_IMAGE_MUTATION_MANIFEST,
  type Json,
} from './v3014-raster-image-projection.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-raster-image-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-raster-image-oracle.json');
const manifestPath = join(scriptDir, 'fixtures/v3014-raster-image-fixtures.json');
const runnerPath = join(scriptDir, 'v3014-raster-image-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-raster-image-projection.ts');
const generatorPath = join(scriptDir, 'generate-v3014-raster-image-fixtures.ts');
const rustServerPath = join(resolveCargoReleaseDir(repoRoot), 'citra-mcp-server');
const outputIndex = process.argv.indexOf('--output');
const outputPath = outputIndex >= 0 ? process.argv[outputIndex + 1] : undefined;
const sha256 = (value: Uint8Array | string): string => createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => {
  const result = spawnSync('git', args, { cwd: repoRoot });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};
const same = (left: Json, right: Json): boolean => JSON.stringify(left) === JSON.stringify(right);

type Case = { id: string; fixture: string; input: Record<string, unknown> };
type Corpus = {
  envelope: { fixtureCount: number; caseCount: number; maxPagesPerCase: number; maxImagesPerCase: number; maxDecodedPixelsPerImage: number };
  nonclaims: Record<string, boolean>;
  cases: Case[];
};
type Oracle = {
  baseline: {
    tag: string; commit: string; tree: string; bunLockSha256: string; runnerSha256: string;
    projectionSha256: string; generatorSha256: string; corpusSha256: string;
    fixtureManifestSha256: string; fixtureSha256: Record<string, string>;
    envelope: Corpus['envelope']; nonclaims: Record<string, boolean>; entrypointSha256: Record<string, string>;
  };
  expectations: Record<string, Json>;
};
const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as Corpus;
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as {
  fixtures: Array<{ path: string; bytes: number; sha256: string; pageCount: number }>;
};

const commit = git('rev-list', '-n', '1', oracle.baseline.tag).toString().trim();
if (commit !== oracle.baseline.commit || commit !== '92651c79c6ce8d10dfa3c76332176c26f222bd78') throw new Error('v3.0.14 raster-image baseline tag moved');
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== oracle.baseline.tree) throw new Error('v3.0.14 raster-image baseline tree mismatch');
const bindings: Array<[Uint8Array, string, string]> = [
  [git('show', `${commit}:bun.lock`), oracle.baseline.bunLockSha256, 'bun lock'],
  [readFileSync(runnerPath), oracle.baseline.runnerSha256, 'runner'],
  [readFileSync(projectionPath), oracle.baseline.projectionSha256, 'projection'],
  [readFileSync(generatorPath), oracle.baseline.generatorSha256, 'generator'],
  [readFileSync(corpusPath), oracle.baseline.corpusSha256, 'corpus'],
  [readFileSync(manifestPath), oracle.baseline.fixtureManifestSha256, 'fixture manifest'],
];
for (const [bytes, expected, label] of bindings) if (sha256(bytes) !== expected) throw new Error(`raster-image ${label} digest mismatch`);
for (const [path, expected] of Object.entries(oracle.baseline.entrypointSha256)) if (sha256(git('show', `${commit}:${path}`)) !== expected) throw new Error(`raster-image TS source mismatch: ${path}`);
if (manifest.fixtures.length !== 3 || corpus.envelope.fixtureCount !== 3 || corpus.envelope.caseCount !== 14 || corpus.envelope.maxPagesPerCase !== 4 || corpus.envelope.maxImagesPerCase !== 2 || corpus.envelope.maxDecodedPixelsPerImage !== 4) throw new Error('raster-image corpus envelope changed');
if (JSON.stringify(corpus.envelope) !== JSON.stringify(oracle.baseline.envelope) || JSON.stringify(corpus.nonclaims) !== JSON.stringify(oracle.baseline.nonclaims)) throw new Error('raster-image envelope/nonclaims differ from oracle binding');
if (corpus.nonclaims.dropInFor3014 !== false || corpus.nonclaims.visualEnrichments !== false) throw new Error('raster-image product nonclaims weakened');
for (const fixture of manifest.fixtures) {
  const path = join(repoRoot, fixture.path);
  if (!existsSync(path) || sha256(readFileSync(path)) !== fixture.sha256 || oracle.baseline.fixtureSha256[fixture.path] !== fixture.sha256) throw new Error(`raster-image fixture identity mismatch: ${fixture.path}`);
}
const ids = corpus.cases.map((entry) => entry.id);
if (new Set(ids).size !== 14 || JSON.stringify(Object.keys(oracle.expectations)) !== JSON.stringify(ids)) throw new Error('raster-image corpus/oracle IDs differ');
if (!existsSync(rustServerPath)) throw new Error('missing release Rust MCP server');

const materialize = (entry: Case): Record<string, unknown> => {
  const input = structuredClone(entry.input);
  const sources = input.sources as Array<Record<string, unknown>>;
  const source = sources[0];
  if (!source) throw new Error(`${entry.id} lacks source`);
  source.path = join(fixtureDir, entry.fixture);
  return input;
};
const observations: Record<string, Json> = {};
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
const child = spawn(rustServerPath, [], {
  cwd: repoRoot,
  stdio: ['pipe', 'pipe', 'pipe'],
  env: { ...process.env, MCP_TRANSPORT: 'stdio' },
}) as ChildProcessWithoutNullStreams;
let buffer = '';
let stderr = '';
const pending = new Map<number, (value: Record<string, unknown>) => void>();
child.stderr.on('data', (chunk: Buffer) => { stderr += chunk.toString(); });
child.stdout.on('data', (chunk: Buffer) => {
  buffer += chunk.toString();
  while (buffer.includes('\n')) {
    const index = buffer.indexOf('\n');
    const line = buffer.slice(0, index).trim();
    buffer = buffer.slice(index + 1);
    if (!line) continue;
    const response = JSON.parse(line) as Record<string, unknown>;
    const id = Number(response.id);
    pending.get(id)?.(response);
    pending.delete(id);
  }
});
const request = (id: number, method: string, params: unknown): Promise<Record<string, unknown>> =>
  new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Rust raster MCP request ${String(id)} timed out: ${stderr.slice(-2000)}`));
    }, 60_000);
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
    clientInfo: { name: 'v3014-raster-image-differential', version: '1' },
  });
  child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
  for (const [index, entry] of corpus.cases.entries()) {
    const response = await request(index + 10, 'tools/call', {
      name: 'read_pdf',
      arguments: materialize(entry),
    });
    if (response.error) throw new Error(`${entry.id}: ${JSON.stringify(response.error)}`);
    const actual = canonicalRasterImageResult(response.result);
    observations[entry.id] = actual;
    const expected = oracle.expectations[entry.id]!;
    if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
  }
} finally {
  child.kill('SIGTERM');
}

const leaves = (value: Json, prefix: Array<string | number> = []): Array<Array<string | number>> => {
  if (Array.isArray(value)) return value.flatMap((entry, index) => leaves(entry, [...prefix, index]));
  if (value && typeof value === 'object') return Object.entries(value).flatMap(([key, entry]) => leaves(entry, [...prefix, key]));
  return [prefix];
};
const mutate = (value: Json, path: Array<string | number>): Json => {
  const changed = structuredClone(value); let cursor = changed as never;
  for (const segment of path.slice(0, -1)) cursor = cursor[segment as never];
  const key = path.at(-1)!; const original = cursor[key as never] as Json;
  cursor[key as never] = (typeof original === 'string' ? `${original}-mutated` : typeof original === 'number' ? original + 1 : original === null ? 'mutated' : !original) as never;
  return changed;
};
let leafMutationCount = 0;
const leafPaths: Record<string, Array<Array<string | number>>> = {};
for (const [id, expectation] of Object.entries(oracle.expectations)) {
  leafPaths[id] = leaves(expectation);
  for (const path of leafPaths[id]!) {
    if (same(expectation, mutate(expectation, path))) throw new Error(`raster-image comparator missed ${id}.${path.join('.')}`);
    leafMutationCount += 1;
  }
}

const grayExpected = oracle.expectations['device-gray-2x2'] as Record<string, Json>;
const grayExpectedImage = (grayExpected.images as Array<Record<string, Json>>)[0]!;
const grayDecoded = grayExpectedImage.decoded as Record<string, Json>;
const grayPng = new PNG({ width: grayExpectedImage.width as number, height: grayExpectedImage.height as number });
grayPng.data = Buffer.from(grayDecoded.rgba as number[]);
const gray = {
  num_pages: grayExpected.num_pages,
  images: [{
    page: grayExpectedImage.page,
    index: grayExpectedImage.index,
    width: grayExpectedImage.width,
    height: grayExpectedImage.height,
    format: grayExpectedImage.format,
    data: PNG.sync.write(grayPng).toString('base64'),
  }],
} as Record<string, unknown>;
const mixedExpectedForProbes = oracle.expectations['mixed-text-image-document-twin'] as Record<string, Json>;
const mixed = {
  num_pages: mixedExpectedForProbes.num_pages,
  elements: structuredClone(mixedExpectedForProbes.elements),
  chunks: structuredClone(mixedExpectedForProbes.chunks),
} as Record<string, unknown>;
const rejects = (source: Record<string, unknown>, change: (value: Record<string, unknown>) => void, label: string): void => {
  const changed = structuredClone(source); change(changed);
  try { canonicalRasterImageResult(changed); } catch { return; }
  throw new Error(`raster-image strict projection accepted ${label}`);
};
const grayImage = (value: Record<string, unknown>) => (value.images as Array<Record<string, unknown>>)[0]!;
const mixedElement = (value: Record<string, unknown>) => (value.elements as Array<Record<string, unknown>>)[0]!;
const mixedChunk = (value: Record<string, unknown>) => (value.chunks as Array<Record<string, unknown>>)[0]!;
const wrongType = [
  [gray, (value: Record<string, unknown>) => { grayImage(value).page = '1'; }, 'images[0].page'],
  [gray, (value: Record<string, unknown>) => { grayImage(value).format = 7; }, 'images[0].format'],
  [gray, (value: Record<string, unknown>) => { grayImage(value).data = 7; }, 'images[0].data'],
  [mixed, (value: Record<string, unknown>) => { mixedElement(value).id = 7; }, 'elements[0].id'],
  [mixed, (value: Record<string, unknown>) => { (mixedChunk(value).element_ids as unknown[])[0] = 7; }, 'chunks[0].element_ids[0]'],
] as const;
for (const [source, change, label] of wrongType) rejects(source, change, label);
const unexpected = [
  [gray, (value: Record<string, unknown>) => { grayImage(value).unexpected = true; }, 'image'],
  [mixed, (value: Record<string, unknown>) => { mixedElement(value).unexpected = true; }, 'element'],
  [mixed, (value: Record<string, unknown>) => { mixedChunk(value).unexpected = true; }, 'chunk'],
] as const;
for (const [source, change, label] of unexpected) rejects(source, change, label);
if (JSON.stringify(wrongType.map((entry) => entry[2])) !== JSON.stringify(RASTER_IMAGE_MUTATION_MANIFEST.wrongPrimitiveTypes) || JSON.stringify(unexpected.map((entry) => entry[2])) !== JSON.stringify(RASTER_IMAGE_MUTATION_MANIFEST.unexpectedFields)) throw new Error('raster-image executed probes differ from mutation manifest');

const imageFreeOmitted = !Object.hasOwn(observations['image-free-omits-images'] as object, 'images');
const includeFalseOmitted = !Object.hasOwn(observations['include-images-false-control'] as object, 'images');
const ancestorShadowed = !Object.hasOwn(observations['direct-resources-shadow-ancestor-xobject'] as object, 'images');
const malformedOmitted = !Object.hasOwn(observations['malformed-zero-width-unsupported-color-space-fails-closed'] as object, 'images');
const mixedExpected = oracle.expectations['mixed-text-image-document-twin'] as Record<string, Json>;
const ast = mixedExpected.document_ast as Record<string, Json>;
const map = mixedExpected.document_map as Record<string, Json>;
if (ast.image_count !== 1 || map.image_element_count !== 1 || !String(mixedExpected.markdown).includes('[Image 1: 2x2 rgb]') || !String(mixedExpected.html).includes('data-image-index="0"')) throw new Error('raster-image downstream Document Twin linkage oracle weakened');
const repeated = oracle.expectations['same-rgb-painted-twice'] as Record<string, Json>;
const repeatedImages = repeated.images as Array<Record<string, Json>>;
if (repeatedImages.length !== 2 || (repeatedImages[0]!.decoded as Record<string, Json>).rgba_sha256 !== (repeatedImages[1]!.decoded as Record<string, Json>).rgba_sha256) throw new Error('raster-image repeated-paint oracle weakened');

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_raster_image_result',
  candidateSha,
  baselineCommit: commit,
  baselineTree: oracle.baseline.tree,
  corpusSha256: sha256(readFileSync(corpusPath)),
  oracleSha256: sha256(readFileSync(oraclePath)),
  runnerSha256: sha256(readFileSync(runnerPath)),
  projectionSha256: sha256(readFileSync(projectionPath)),
  generatorSha256: sha256(readFileSync(generatorPath)),
  fixtureManifestSha256: sha256(readFileSync(manifestPath)),
  fixtureSha256: oracle.baseline.fixtureSha256,
  entrypointSha256: oracle.baseline.entrypointSha256,
  envelope: corpus.envelope,
  caseCount: ids.length,
  passed: ids.length - failures.length,
  skipped: 0,
  mutationSensitive: {
    allClaimedFields: true,
    manifestVersion: RASTER_IMAGE_MUTATION_MANIFEST.version,
    mutationManifestSha256: sha256(JSON.stringify({ ...RASTER_IMAGE_MUTATION_MANIFEST, leafPaths })),
    leafMutationCount,
    wrongPrimitiveTypeProbeCount: wrongType.length,
    unexpectedFieldProbeCount: unexpected.length,
    requiredOmissionProbeCount: 2,
  },
  decodedPixelProof: { comparedCompressionBytes: false, maxPixelsPerImage: 4, repeatedPaintPixelIdentity: true },
  omissionProof: { imageFreeOmitted, includeFalseOmitted, ancestorShadowed, malformedOmitted },
  capabilityStatus: { includeImages: 'PARTIAL', visualEnrichments: 'PARTIAL' },
  nonclaims: corpus.nonclaims,
  productTruth: { dropInFor3014: false, publishFreeze: true },
  pass: failures.length === 0,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
console.error(`v3.0.14 raster-image differential: PASS (${String(ids.length)}/${String(ids.length)}, zero skipped)`);
