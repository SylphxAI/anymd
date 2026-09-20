#!/usr/bin/env bun

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalSelectableReadResult, canonicalSelectableSearchResult, SELECTABLE_TEXT_SEGMENTATION_MUTATION_MANIFEST, type Json } from './v3014-selectable-text-segmentation-projection.ts';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-selectable-text-segmentation-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-selectable-text-segmentation-oracle.json');
const manifestPath = join(scriptDir, 'fixtures/v3014-selectable-text-segmentation-fixture.json');
const runnerPath = join(scriptDir, 'v3014-selectable-text-segmentation-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-selectable-text-segmentation-projection.ts');
const generatorPath = join(scriptDir, 'generate-v3014-selectable-text-segmentation-fixture.ts');
const rustCliPath = join(resolveCargoReleaseDir(repoRoot), 'pdf-reader-cli');
const outputIndex = process.argv.indexOf('--output');
const outputPath = outputIndex >= 0 ? process.argv[outputIndex + 1] : undefined;
const sha256 = (value: Uint8Array | string): string => createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => { const result = spawnSync('git', args, { cwd: repoRoot }); if (result.status !== 0) throw new Error(result.stderr.toString()); return result.stdout; };
const same = (left: Json, right: Json): boolean => JSON.stringify(left) === JSON.stringify(right);
type Case = { id: string; operation: 'read' | 'search'; fixture: string; input: Record<string, unknown> };
type Corpus = { envelope: { fixtureCount: number; pageCount: number; caseCount: number; maxPagesPerCase: number }; nonclaims: Record<string, boolean>; cases: Case[] };
type Oracle = { baseline: { tag: string; commit: string; tree: string; bunLockSha256: string; entrypointSha256: Record<string, string>; [key: string]: unknown }; expectations: Record<string, Json> };
const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as Corpus;
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as { fixture: { path: string; sha256: string; bytes: number; pageCount: number }; primitiveContract: Record<string, number> };

const commit = git('rev-list', '-n', '1', oracle.baseline.tag).toString().trim();
if (commit !== oracle.baseline.commit || commit !== '92651c79c6ce8d10dfa3c76332176c26f222bd78') throw new Error('v3.0.14 selectable-text-segmentation tag moved');
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== oracle.baseline.tree) throw new Error('v3.0.14 selectable-text-segmentation tree mismatch');
if (sha256(git('show', `${commit}:bun.lock`)) !== oracle.baseline.bunLockSha256) throw new Error('baseline lock mismatch');
for (const [bytes, expected, label] of [
  [readFileSync(runnerPath), oracle.baseline.runnerSha256, 'runner'],
  [readFileSync(projectionPath), oracle.baseline.projectionSha256, 'projection'],
  [readFileSync(generatorPath), oracle.baseline.generatorSha256, 'generator'],
  [readFileSync(corpusPath), oracle.baseline.corpusSha256, 'corpus'],
  [readFileSync(manifestPath), oracle.baseline.fixtureManifestSha256, 'fixture manifest'],
] as Array<[Uint8Array, unknown, string]>) if (sha256(bytes) !== expected) throw new Error(`${label} digest mismatch`);
for (const [path, digest] of Object.entries(oracle.baseline.entrypointSha256)) if (sha256(git('show', `${commit}:${path}`)) !== digest) throw new Error(`baseline source mismatch: ${path}`);
const fixturePath = join(repoRoot, manifest.fixture.path);
if (!existsSync(fixturePath) || readFileSync(fixturePath).length !== manifest.fixture.bytes || sha256(readFileSync(fixturePath)) !== manifest.fixture.sha256) throw new Error('fixture identity mismatch');
if (manifest.fixture.sha256 !== oracle.baseline.fixtureSha256) throw new Error('fixture baseline mismatch');
if (corpus.cases.length !== 4 || corpus.envelope.fixtureCount !== 1 || corpus.envelope.pageCount !== 5 || corpus.envelope.caseCount !== 4 || corpus.envelope.maxPagesPerCase !== 5) throw new Error('corpus envelope changed');
if (Object.values(corpus.nonclaims).some((value) => value !== false)) throw new Error('nonclaims weakened');
const ids = corpus.cases.map((entry) => entry.id);
if (JSON.stringify(Object.keys(oracle.expectations)) !== JSON.stringify(ids)) throw new Error('corpus/oracle IDs differ');
if (!existsSync(rustCliPath)) throw new Error('missing release Rust CLI');

const observations: Record<string, Json> = {};
const raw: Record<string, Record<string, unknown>> = {};
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
for (const entry of corpus.cases) {
  const input = structuredClone(entry.input);
  if (entry.operation === 'read') {
    const source = (input.sources as Array<Record<string, unknown>>)[0]!;
    source.path = join(fixtureDir, entry.fixture);
  } else {
    const pages = input.pages; delete input.pages;
    input.sources = [{ path: join(fixtureDir, entry.fixture), ...(pages !== undefined ? { pages } : {}) }];
  }
  const process = spawnSync(rustCliPath, [], { cwd: repoRoot, encoding: 'utf8', maxBuffer: 100 * 1024 * 1024, input: JSON.stringify({ tool: entry.operation === 'read' ? 'read_pdf' : 'search_pdf', input }) });
  if (process.status !== 0 || !process.stdout) throw new Error(process.stderr || `${entry.id} subprocess failed`);
  const envelope = JSON.parse(process.stdout) as Record<string, unknown>;
  const result = envelope.result as { content?: Array<{ text?: string }> } | undefined;
  const text = result?.content?.[0]?.text;
  if (!text) throw new Error(`${entry.id} lacks structured result text`);
  const payload = JSON.parse(text) as { results: Array<Record<string, unknown>> };
  const sourceResult = payload.results[0]!;
  const sourceData = entry.operation === 'read' ? sourceResult.data as Record<string, unknown> : sourceResult;
  raw[entry.id] = sourceData;
  const actual = entry.operation === 'read' ? canonicalSelectableReadResult(sourceData) : canonicalSelectableSearchResult(sourceData);
  observations[entry.id] = actual;
  const expected = oracle.expectations[entry.id]!;
  if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
}

const leaves = (value: Json, prefix: Array<string | number> = []): Array<Array<string | number>> => Array.isArray(value) ? value.flatMap((entry, index) => leaves(entry, [...prefix, index])) : value && typeof value === 'object' ? Object.entries(value).flatMap(([key, entry]) => leaves(entry, [...prefix, key])) : [prefix];
const mutate = (value: Json, path: Array<string | number>): Json => { const changed = structuredClone(value); let cursor = changed as never; for (const segment of path.slice(0, -1)) cursor = cursor[segment as never]; const key = path.at(-1)!; const original = cursor[key as never] as Json; cursor[key as never] = (typeof original === 'string' ? `${original}-mutated` : typeof original === 'number' ? original + 1 : original === null ? 'mutated' : !original) as never; return changed; };
let leafMutationCount = 0;
const leafPaths: Record<string, Array<Array<string | number>>> = {};
for (const [id, expectation] of Object.entries(oracle.expectations)) { leafPaths[id] = leaves(expectation); for (const path of leafPaths[id]!) { if (same(expectation, mutate(expectation, path))) throw new Error(`comparator missed ${id}.${path.join('.')}`); leafMutationCount += 1; } }

const exposed = raw['document-twin-exposed-segmentation']!;
const mapOnly = raw['document-map-only-hidden-segmentation-dependencies']!;
const gapRaw = raw['search-gap-boundary']!;
const roundedRaw = raw['search-rounded-y-boundary']!;
const rejectRead = (source: Record<string, unknown>, change: (value: Record<string, unknown>) => void, label: string): void => { const changed = structuredClone(source); change(changed); try { canonicalSelectableReadResult(changed); } catch { return; } throw new Error(`strict read projection accepted ${label}`); };
const rejectSearch = (source: Record<string, unknown>, change: (value: Record<string, unknown>) => void, label: string): void => { const changed = structuredClone(source); change(changed); try { canonicalSelectableSearchResult(changed); } catch { return; } throw new Error(`strict search projection accepted ${label}`); };
const firstLine = (value: Record<string, unknown>): Record<string, unknown> => (((value.text_layer as Record<string, unknown>).pages as Array<Record<string, unknown>>)[0]!.lines as Array<Record<string, unknown>>)[0]!;
const firstRun = (value: Record<string, unknown>): Record<string, unknown> => (firstLine(value).runs as Array<Record<string, unknown>>)[0]!;
const firstChar = (value: Record<string, unknown>): Record<string, unknown> => (firstRun(value).chars as Array<Record<string, unknown>>)[0]!;
const firstMapPage = (value: Record<string, unknown>): Record<string, unknown> => ((value.document_map as Record<string, unknown>).pages as Array<Record<string, unknown>>)[0]!;
const firstSearchMatch = (value: Record<string, unknown>): Record<string, unknown> => (value.matches as Array<Record<string, unknown>>)[0]!;
rejectRead(exposed, (value) => { firstLine(value).char_start = '0'; }, 'line.char_start wrong type');
rejectRead(exposed, (value) => { (firstRun(value).bounding_box as Record<string, unknown>).left = '72'; }, 'run box wrong type');
rejectRead(exposed, (value) => { firstMapPage(value).text_item_count = '1'; }, 'map count wrong type');
rejectSearch(roundedRaw, (value) => { (firstSearchMatch(value).bounding_box as Record<string, unknown>).right = '77'; }, 'search box wrong type');
rejectSearch(roundedRaw, (value) => { value.total_matches = '1'; }, 'search total wrong type');
rejectRead(exposed, (value) => { firstLine(value).unexpected = true; }, 'line unexpected field');
rejectRead(exposed, (value) => { firstRun(value).unexpected = true; }, 'run unexpected field');
rejectRead(exposed, (value) => { firstMapPage(value).unexpected = true; }, 'map page unexpected field');
rejectSearch(roundedRaw, (value) => { firstSearchMatch(value).unexpected = true; }, 'search match unexpected field');
rejectRead(exposed, (value) => { delete firstLine(value).text; }, 'line.text omission');
rejectRead(exposed, (value) => { delete firstRun(value).text; }, 'run.text omission');
rejectRead(exposed, (value) => { delete firstChar(value).bounding_box; }, 'char box omission');
rejectRead(exposed, (value) => { delete firstMapPage(value).element_ids; }, 'map element ids omission');
rejectSearch(roundedRaw, (value) => { delete firstSearchMatch(value).bounding_box; }, 'search box omission');
let publicOmissionProbeCount = 0;
for (const key of ['elements', 'chunks', 'text_layer', 'page_contents']) { if (Object.hasOwn(mapOnly, key)) throw new Error(`map-only leaked ${key}`); publicOmissionProbeCount += 1; }
let dependencyPresenceProbeCount = 0;
const exposedCanonical = canonicalSelectableReadResult(exposed);
for (const key of SELECTABLE_TEXT_SEGMENTATION_MUTATION_MANIFEST.dependencyPresence) { const changed = structuredClone(exposed); if (Object.hasOwn(changed, key)) delete changed[key]; else changed[key] = []; if (same(canonicalSelectableReadResult(changed), exposedCanonical)) throw new Error(`dependency presence undetected: ${key}`); dependencyPresenceProbeCount += 1; }

const expected = oracle.expectations['document-twin-exposed-segmentation'] as Record<string, Json>;
const expectedPages = ((expected.text_layer as Record<string, Json>).pages as Array<Record<string, Json>>);
const expectedElements = expected.elements as Array<Record<string, Json>>;
const mapPages = ((expected.document_map as Record<string, Json>).pages as Array<Record<string, Json>>);
const gap = oracle.expectations['search-gap-boundary'] as Record<string, Json>;
const rounded = oracle.expectations['search-rounded-y-boundary'] as Record<string, Json>;
const gapAcceptedRuns = ((((expectedPages[1]?.lines as Array<Record<string, Json>>)[0]?.runs) ?? []) as Array<Record<string, Json>>);
const gapRejectedFirstRun = ((((expectedPages[2]?.lines as Array<Record<string, Json>>)[0]?.runs) ?? []) as Array<Record<string, Json>>)[0];
const gapRejectedSecondRun = ((((expectedPages[2]?.lines as Array<Record<string, Json>>)[1]?.runs) ?? []) as Array<Record<string, Json>>)[0];
const right = (entry: Record<string, Json> | undefined): number => ((entry?.bounding_box as Record<string, Json> | undefined)?.right ?? Number.NaN) as number;
const left = (entry: Record<string, Json> | undefined): number => ((entry?.bounding_box as Record<string, Json> | undefined)?.left ?? Number.NaN) as number;
const semanticProof = {
  ltrPhysicalOrder: expectedElements[0]?.content === 'ABC',
  multipartRunOffsets: JSON.stringify(((expectedPages[0]?.lines as Array<Record<string, Json>>)[0]?.runs as Array<Record<string, Json>>).map((entry) => entry.text)) === JSON.stringify(['A', 'B', 'C']),
  exact48Joined: expectedElements[1]?.content === 'GAPJOIN' && gapAcceptedRuns.length === 2 && left(gapAcceptedRuns[1]) - right(gapAcceptedRuns[0]) === 48,
  gapAbove48Split: expectedElements[2]?.content === 'GAP' && expectedElements[3]?.content === 'JOIN' && Math.round((left(gapRejectedSecondRun) - right(gapRejectedFirstRun)) * 1000) / 1000 === 48.001,
  roundedHalfJoined: expectedElements[4]?.content === 'YJOIN',
  belowRoundedHalfSplit: expectedElements[5]?.content === 'Y' && expectedElements[6]?.content === 'JOIN',
  documentMapSegmentationCounts: JSON.stringify(mapPages.map((page) => page.text_item_count)) === JSON.stringify([1, 1, 2, 1, 2]),
  mapOnlyDependenciesHidden: !Object.hasOwn(mapOnly, 'elements') && !Object.hasOwn(mapOnly, 'chunks') && !Object.hasOwn(mapOnly, 'text_layer') && !Object.hasOwn(mapOnly, 'page_contents'),
  gapSearchDoesNotCrossSplit: gap.total_matches === 1 && ((gap.matches as Json[])[0] as Record<string, Json>).page === 2,
  roundedSearchDoesNotCrossSplit: rounded.total_matches === 1 && ((rounded.matches as Json[])[0] as Record<string, Json>).page === 4,
};
if (Object.values(semanticProof).some((value) => !value)) throw new Error('semantic proof incomplete');

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
const mutationManifestSha256 = sha256(JSON.stringify({ ...SELECTABLE_TEXT_SEGMENTATION_MUTATION_MANIFEST, leafPaths }));
const report = {
  schemaVersion: 1, profile: 'pdf_reader_v3014_selectable_text_segmentation_result', candidateSha,
  baselineCommit: commit, baselineTree: oracle.baseline.tree,
  corpusSha256: sha256(readFileSync(corpusPath)), oracleSha256: sha256(readFileSync(oraclePath)), runnerSha256: sha256(readFileSync(runnerPath)), projectionSha256: sha256(readFileSync(projectionPath)), generatorSha256: sha256(readFileSync(generatorPath)), fixtureManifestSha256: sha256(readFileSync(manifestPath)), fixtureSha256: manifest.fixture.sha256,
  entrypointSha256: oracle.baseline.entrypointSha256, envelope: corpus.envelope,
  caseCount: ids.length, passed: ids.length - failures.length, skipped: 0,
  mutationSensitive: { allClaimedFields: true, manifestVersion: SELECTABLE_TEXT_SEGMENTATION_MUTATION_MANIFEST.version, mutationManifestSha256, leafMutationCount, wrongPrimitiveTypeProbeCount: 5, unexpectedFieldProbeCount: 4, requiredOmissionProbeCount: 5, publicOmissionProbeCount, dependencyPresenceProbeCount },
  semanticProof, pdfjsObservation: { syntheticWhitespaceAt48: false, syntheticWhitespaceAbove48: false, directGapThresholdIsolatedByPdfFixture: true },
  nonclaims: corpus.nonclaims, productTruth: { dropInFor3014: false, publishFreeze: true }, pass: failures.length === 0, failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) writeFileSync(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
console.error(`v3.0.14 selectable-text-segmentation differential: PASS (${String(ids.length)}/${String(ids.length)}, zero skipped)`);
