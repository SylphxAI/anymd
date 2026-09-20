#!/usr/bin/env bun

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';
  canonicalDocumentMapResult,
  DOCUMENT_MAP_DEPENDENCY_SURFACES,
  DOCUMENT_MAP_MUTATION_MANIFEST,
  type Json,
} from './v3014-document-map-projection.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-document-map-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-document-map-oracle.json');
const fixtureManifestPath = join(scriptDir, 'fixtures/v3014-document-map-fixture.json');
const runnerPath = join(scriptDir, 'v3014-document-map-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-document-map-projection.ts');
const generatorPath = join(scriptDir, 'generate-v3014-document-map-fixture.ts');
const rustCliPath = join(resolveCargoReleaseDir(repoRoot), 'pdf-reader-cli');
const outputFlag = process.argv.indexOf('--output');
const outputPath = outputFlag >= 0 ? process.argv[outputFlag + 1] : undefined;

type Case = { id: string; input: Record<string, unknown> };
type Oracle = {
  baseline: {
    tag: string; commit: string; tree: string; bunLockSha256: string;
    runnerSha256: string; projectionSha256: string; generatorSha256: string;
    corpusSha256: string; fixtureManifestSha256: string; fixtureSha256: string;
    entrypointSha256: Record<string, string>;
  };
  expectations: Record<string, Json>;
};
const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as { cases: Case[] };
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const fixtureManifest = JSON.parse(readFileSync(fixtureManifestPath, 'utf8')) as {
  fixture: { path: string; bytes: number; sha256: string };
};
const sha256 = (value: Uint8Array | string): string =>
  createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => {
  const result = spawnSync('git', args, { cwd: repoRoot });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};
const canonicalJson = (value: Json): Json => {
  if (Array.isArray(value)) return value.map(canonicalJson);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b))
      .map(([key, entry]) => [key, canonicalJson(entry)]));
  }
  return value;
};
const same = (left: Json, right: Json): boolean =>
  JSON.stringify(canonicalJson(left)) === JSON.stringify(canonicalJson(right));
const rawSame = (left: unknown, right: unknown): boolean =>
  JSON.stringify(canonicalJson(left as Json)) === JSON.stringify(canonicalJson(right as Json));

const expectedIds = [
  'map-only-hidden-semantic-chunk-dependencies',
  'map-reuses-exposed-plain-chunks',
  'map-reuses-exposed-semantic-elements-and-chunks',
  'map-reuses-exposed-text-layout-safety-geometry',
  'map-selected-pages-dedupe-sort',
  'map-empty-page-routing',
  'map-prompt-injection-safety-routing',
  'map-hostile-admitted-range-bounded-index',
];
function verifyAuthority(): Record<string, string> {
  const commit = git('rev-list', '-n', '1', oracle.baseline.tag).toString().trim();
  if (commit !== oracle.baseline.commit) throw new Error('baseline tag moved');
  const tree = git('rev-parse', `${commit}^{tree}`).toString().trim();
  if (tree !== oracle.baseline.tree) throw new Error('baseline tree mismatch');
  if (sha256(git('show', `${commit}:bun.lock`)) !== oracle.baseline.bunLockSha256) {
    throw new Error('baseline lock mismatch');
  }
  for (const [path, expected, label] of [
    [runnerPath, oracle.baseline.runnerSha256, 'runner'],
    [projectionPath, oracle.baseline.projectionSha256, 'projection'],
    [generatorPath, oracle.baseline.generatorSha256, 'generator'],
    [corpusPath, oracle.baseline.corpusSha256, 'corpus'],
    [fixtureManifestPath, oracle.baseline.fixtureManifestSha256, 'fixture manifest'],
  ] as const) {
    if (sha256(readFileSync(path)) !== expected) throw new Error(`document-map ${label} mismatch`);
  }
  for (const [sourceFile, expected] of Object.entries(oracle.baseline.entrypointSha256)) {
    if (sha256(git('show', `${commit}:${sourceFile}`)) !== expected) {
      throw new Error(`baseline entrypoint mismatch: ${sourceFile}`);
    }
  }
  const fixturePath = join(repoRoot, fixtureManifest.fixture.path);
  if (!existsSync(fixturePath)) throw new Error('missing document-map fixture');
  const fixtureBytes = readFileSync(fixturePath);
  if (fixtureBytes.length !== fixtureManifest.fixture.bytes ||
      sha256(fixtureBytes) !== fixtureManifest.fixture.sha256 ||
      fixtureManifest.fixture.sha256 !== oracle.baseline.fixtureSha256) {
    throw new Error('document-map fixture digest mismatch');
  }
  if (JSON.stringify(corpus.cases.map(({ id }) => id)) !== JSON.stringify(expectedIds) ||
      JSON.stringify(Object.keys(oracle.expectations).sort()) !== JSON.stringify([...expectedIds].sort())) {
    throw new Error('document-map corpus and oracle must contain the exact eight case IDs');
  }
  const oracleText = JSON.stringify(oracle.expectations);
  for (const field of [
    'version', 'profile', 'layers', 'pages', 'elements', 'chunks', 'layout_diagnostics',
    'safety_findings', 'routing', 'summary', 'geometry', 'element_ids', 'chunk_ids',
    'text_layer_page_index', 'text_chars', 'text_item_count', 'needs_ocr_pages',
    'low_confidence_pages', 'selected_pages', 'processed_page_count', 'chunk_count',
    'safety_finding_count', 'prompt_injection_pattern',
  ]) {
    if (!oracleText.includes(`"${field}"`)) throw new Error(`oracle lacks claimed map field: ${field}`);
  }
  return {
    baselineCommit: commit,
    baselineTree: tree,
    corpusSha256: sha256(readFileSync(corpusPath)),
    oracleSha256: sha256(readFileSync(oraclePath)),
    runnerSha256: sha256(readFileSync(runnerPath)),
    projectionSha256: sha256(readFileSync(projectionPath)),
    generatorSha256: sha256(readFileSync(generatorPath)),
    fixtureManifestSha256: sha256(readFileSync(fixtureManifestPath)),
    fixtureSha256: fixtureManifest.fixture.sha256,
  };
}
function materialize(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(materialize);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value as Record<string, unknown>).map(([key, entry]) => [
      key === 'fixture' ? 'path' : key,
      key === 'fixture' && typeof entry === 'string' ? join(fixtureDir, entry) : materialize(entry),
    ]));
  }
  return value;
}
function leafPaths(value: Json, prefix: Array<string | number> = []): Array<Array<string | number>> {
  if (Array.isArray(value)) return value.flatMap((entry, index) => leafPaths(entry, [...prefix, index]));
  if (value && typeof value === 'object') {
    return Object.entries(value).flatMap(([key, entry]) => leafPaths(entry, [...prefix, key]));
  }
  return [prefix];
}
function mutateAt(value: Json, path: Array<string | number>): Json {
  const mutated = structuredClone(value);
  let cursor = mutated as Json;
  for (const segment of path.slice(0, -1)) cursor = (cursor as never)[segment as never];
  const key = path.at(-1)!;
  const original = (cursor as never)[key as never] as Json;
  const replacement: Json = typeof original === 'string' ? `${original}-mutated`
    : typeof original === 'number' ? original + 1
    : typeof original === 'boolean' ? !original : original === null ? 'mutated' : 'mutated';
  (cursor as never)[key as never] = replacement as never;
  return mutated;
}

const authority = verifyAuthority();
if (!existsSync(rustCliPath)) throw new Error('missing release Rust CLI');
const requireSuccessfulSubprocess = (
  result: ReturnType<typeof spawnSync>,
  label: string
): void => {
  if (result.status !== 0) throw new Error(result.stderr?.toString() || `${label} exited nonzero`);
};
let subprocessNonzeroRejected = false;
try {
  requireSuccessfulSubprocess(
    spawnSync(process.execPath, ['-e', 'process.exit(17)'], { encoding: 'utf8' }),
    'nonzero rejection probe'
  );
} catch {
  subprocessNonzeroRejected = true;
}
if (!subprocessNonzeroRejected) throw new Error('nonzero subprocess was not rejected');
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
const observations: Record<string, Json> = {};
const rawResults: Record<string, Record<string, unknown>> = {};
for (const entry of corpus.cases) {
  const result = spawnSync(rustCliPath, [], {
    cwd: repoRoot, encoding: 'utf8',
    input: JSON.stringify({ tool: 'read_pdf', input: materialize(entry.input) }),
    maxBuffer: 120 * 1024 * 1024,
  });
  requireSuccessfulSubprocess(result, `Rust document-map case: ${entry.id}`);
  const envelope = JSON.parse(result.stdout) as Record<string, unknown>;
  const text = (envelope.result as { content?: Array<{ text?: string }> } | undefined)?.content?.[0]?.text;
  if (!text) throw new Error('Rust CLI response lacks structured text');
  const payload = JSON.parse(text) as { results: Array<{ success: boolean; data: Record<string, unknown> }> };
  if (!payload.results[0]?.success) throw new Error(`Rust document-map case failed: ${entry.id}`);
  rawResults[entry.id] = payload.results[0].data;
  const actual = canonicalDocumentMapResult(payload.results[0].data);
  const expected = oracle.expectations[entry.id]!;
  observations[entry.id] = actual;
  if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
}

let leafMutationCount = 0;
for (const [caseId, expectation] of Object.entries(oracle.expectations)) {
  const paths = leafPaths(expectation);
  const missed = paths.filter((path) => same(mutateAt(expectation, path), expectation));
  if (missed.length > 0) throw new Error(`comparison missed ${missed.length} leaf mutations in ${caseId}`);
  leafMutationCount += paths.length;
}
const baseRaw = rawResults['map-only-hidden-semantic-chunk-dependencies']!;
const emptyRaw = rawResults['map-empty-page-routing']!;
const richRaw = rawResults['map-reuses-exposed-text-layout-safety-geometry']!;
const assertRejects = (mutate: (value: Record<string, unknown>) => void, label: string, source = baseRaw) => {
  const mutated = structuredClone(source);
  mutate(mutated);
  try { canonicalDocumentMapResult(mutated); } catch { return; }
  throw new Error(`strict projection accepted invalid mutation: ${label}`);
};
const map = (value: Record<string, unknown>) => value.document_map as Record<string, unknown>;
const firstPage = (value: Record<string, unknown>) => (map(value).pages as Array<Record<string, unknown>>)[0]!;
const routing = (value: Record<string, unknown>) => map(value).routing as Record<string, unknown>;
const summary = (value: Record<string, unknown>) => map(value).summary as Record<string, unknown>;
assertRejects((value) => { map(value).version = 1; }, 'map.version type');
assertRejects((value) => { firstPage(value).page = '1'; }, 'page.page type');
assertRejects((value) => { (firstPage(value).element_ids as unknown[])[0] = 1; }, 'element id type');
assertRejects((value) => { (routing(value).low_confidence_pages as unknown[])[0] = '4'; }, 'routing page type', emptyRaw);
assertRejects((value) => { summary(value).processed_page_count = '3'; }, 'summary count type');
assertRejects((value) => { (summary(value).selected_pages as unknown[])[0] = '1'; }, 'selected page type');
assertRejects((value) => { map(value).unexpected = true; }, 'unexpected map field');
assertRejects((value) => { firstPage(value).unexpected = true; }, 'unexpected page field');
assertRejects((value) => { routing(value).unexpected = true; }, 'unexpected routing field');
assertRejects((value) => { summary(value).unexpected = true; }, 'unexpected summary field');
assertRejects((value) => { (map(value).elements as Array<Record<string, unknown>>)[0]!.unexpected = true; }, 'unexpected element field');
assertRejects((value) => { (map(value).chunks as Array<Record<string, unknown>>)[0]!.unexpected = true; }, 'unexpected chunk field');
assertRejects((value) => { (map(value).layout_diagnostics as Array<Record<string, unknown>>)[0]!.unexpected = true; }, 'unexpected layout field');
assertRejects((value) => { (map(value).safety_findings as Array<Record<string, unknown>>)[0]!.unexpected = true; }, 'unexpected safety field', richRaw);
assertRejects((value) => { delete map(value).version; }, 'required map version');
assertRejects((value) => { delete map(value).pages; }, 'required map pages');
assertRejects((value) => { delete firstPage(value).page; }, 'required page number');
assertRejects((value) => { delete firstPage(value).element_ids; }, 'required page element ids');
assertRejects((value) => { delete routing(value).needs_ocr_pages; }, 'required routing field');
assertRejects((value) => { delete summary(value).processed_page_count; }, 'required summary field');

const baseExpected = oracle.expectations['map-only-hidden-semantic-chunk-dependencies']!;
let privateLeakProbeCount = 0;
for (const key of DOCUMENT_MAP_MUTATION_MANIFEST.privateLeakage) {
  const mutated = structuredClone(baseRaw);
  mutated[key] = { leaked: true };
  if (same(canonicalDocumentMapResult(mutated), baseExpected)) throw new Error(`private output leakage undetected: ${key}`);
  privateLeakProbeCount += 1;
}
const hostile = observations['map-hostile-admitted-range-bounded-index'] as Record<string, Json>;
const hostileMap = hostile.document_map as Record<string, Json>;
const hostileSummary = hostileMap.summary as Record<string, Json>;
const hostilePages = hostileMap.pages as Json[];
const hostileSpanBounded = hostilePages.length === 5 &&
  JSON.stringify(hostileSummary.selected_pages) === JSON.stringify([1, 2, 3, 4, 5]);
if (!hostileSpanBounded) throw new Error('hostile admitted range materialized pages outside the actual document');

const plainRaw = rawResults['map-reuses-exposed-plain-chunks']!;
const semanticRaw = rawResults['map-reuses-exposed-semantic-elements-and-chunks']!;
const richMap = map(richRaw);
const richPages = richMap.pages as Array<Record<string, unknown>>;
const richTextLayer = richRaw.text_layer as Record<string, unknown>;
const richTextPages = richTextLayer.pages as Array<Record<string, unknown>>;
const richTextSummary = richTextLayer.summary as Record<string, unknown>;
const ordinaryTextPageCountersMatch = richPages.every((page) => {
  const textPage = richTextPages.find((candidate) => candidate.page === page.page);
  if (!textPage) return false;
  const lines = textPage.lines as Array<Record<string, unknown>>;
  const words = lines.flatMap((line) => line.words as Array<Record<string, unknown>>);
  const chars = lines.flatMap((line) => line.chars as Array<Record<string, unknown>>);
  return page.text_layer_line_count === textPage.line_count &&
    page.text_layer_word_count === textPage.word_count &&
    page.text_layer_char_count === textPage.char_count &&
    page.text_layer_lines_with_bounding_boxes === lines.filter((line) => line.bounding_box !== undefined).length &&
    page.text_layer_words_with_bounding_boxes === words.filter((word) => word.bounding_box !== undefined).length &&
    page.text_layer_chars_with_bounding_boxes === chars.filter((char) => char.bounding_box !== undefined).length;
});
const richSummary = richMap.summary as Record<string, unknown>;
const ordinaryTextSummaryCountersMatch =
  richSummary.text_layer_page_count === richTextSummary.page_count &&
  richSummary.text_layer_line_count === richTextSummary.line_count &&
  richSummary.text_layer_word_count === richTextSummary.word_count &&
  richSummary.text_layer_char_count === richTextSummary.char_count &&
  richSummary.text_layer_lines_with_bounding_boxes === richTextSummary.lines_with_bounding_boxes &&
  richSummary.text_layer_words_with_bounding_boxes === richTextSummary.words_with_bounding_boxes &&
  richSummary.text_layer_chars_with_bounding_boxes === richTextSummary.chars_with_bounding_boxes;
const cacheIdentity = {
  exposedPlainChunksReused: rawSame(map(plainRaw).chunks, plainRaw.chunks),
  exposedSemanticElementsReused: rawSame(map(semanticRaw).elements, semanticRaw.elements),
  exposedSemanticChunksReused: rawSame(map(semanticRaw).chunks, semanticRaw.chunks),
  exposedLayoutReused: rawSame(richMap.layout_diagnostics, richRaw.layout_diagnostics),
  exposedSafetyReused: rawSame(richMap.safety_findings, richRaw.safety_findings),
  exposedGeometryReused: rawSame(
    richPages.map((page) => page.geometry),
    richRaw.page_geometry
  ),
  ordinaryTextPageCountersMatch,
  ordinaryTextSummaryCountersMatch,
};
if (Object.values(cacheIdentity).some((value) => !value)) {
  throw new Error(`document-map cache identity mismatch: ${JSON.stringify(cacheIdentity)}`);
}

const mutationManifestSha256 = sha256(JSON.stringify(DOCUMENT_MAP_MUTATION_MANIFEST));
const mutationSensitive = {
  allClaimedFields: true,
  manifestVersion: DOCUMENT_MAP_MUTATION_MANIFEST.version,
  mutationManifestSha256,
  leafMutationCount,
  wrongPrimitiveTypeProbeCount: DOCUMENT_MAP_MUTATION_MANIFEST.wrongPrimitiveTypes.length,
  unexpectedFieldProbeCount: DOCUMENT_MAP_MUTATION_MANIFEST.unexpectedFields.length,
  requiredOmissionProbeCount: DOCUMENT_MAP_MUTATION_MANIFEST.requiredOmissions.length,
  privateLeakProbeCount,
  dependencyPresenceProbeCount: DOCUMENT_MAP_MUTATION_MANIFEST.dependencyPresence.length,
  hostileSpanBounded,
};
const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(`candidate SHA assertion mismatch: expected ${process.env.CANDIDATE_SHA}, executed ${candidateSha}`);
}
const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_document_map_result',
  candidateSha,
  ...authority,
  caseCount: corpus.cases.length,
  passed: corpus.cases.length - failures.length,
  skipped: 0,
  subprocessNonzeroRejected,
  mutationSensitive,
  cacheIdentity,
  pass: failures.length === 0,
  observations,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
