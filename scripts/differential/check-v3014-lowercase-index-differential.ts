#!/usr/bin/env bun

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalLowercaseIndexResult, LOWERCASE_INDEX_MUTATION_MANIFEST, type Json } from './v3014-lowercase-index-projection.ts';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-lowercase-index-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-lowercase-index-oracle.json');
const manifestPath = join(scriptDir, 'fixtures/v3014-lowercase-index-fixture.json');
const runnerPath = join(scriptDir, 'v3014-lowercase-index-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-lowercase-index-projection.ts');
const generatorPath = join(scriptDir, 'generate-v3014-lowercase-index-fixture.ts');
const rustCliPath = join(resolveCargoReleaseDir(repoRoot), 'pdf-reader-cli');
const outputIndex = process.argv.indexOf('--output');
const outputPath = outputIndex >= 0 ? process.argv[outputIndex + 1] : undefined;
const sha256 = (value: Uint8Array | string): string => createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => {
  const result = spawnSync('git', args, { cwd: repoRoot });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};
const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as {
  localeContract: { defaultLocale: string; sentinel: string; lowercase: string };
  nonclaims: Record<string, boolean>;
  cases: Array<{ id: string; fixture: string; input: Record<string, unknown> }>;
};
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as { baseline: Record<string, unknown> & { tag: string; commit: string; tree: string; entrypointSha256: Record<string, string> }; expectations: Record<string, Json> };
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as { fixture: { path: string; bytes: number; sha256: string; pageCount: number } };
const same = (left: Json, right: Json): boolean => JSON.stringify(left) === JSON.stringify(right);
const baseline = oracle.baseline;
const commit = git('rev-list', '-n', '1', baseline.tag).toString().trim();
if (commit !== baseline.commit || commit !== '92651c79c6ce8d10dfa3c76332176c26f222bd78') throw new Error('v3.0.14 lowercase-index baseline tag moved');
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== baseline.tree) throw new Error('v3.0.14 lowercase-index baseline tree mismatch');
const bindings: Array<[Uint8Array, unknown, string]> = [
  [git('show', `${commit}:bun.lock`), baseline.bunLockSha256, 'bun lock'], [readFileSync(runnerPath), baseline.runnerSha256, 'runner'],
  [readFileSync(projectionPath), baseline.projectionSha256, 'projection'], [readFileSync(generatorPath), baseline.generatorSha256, 'generator'],
  [readFileSync(corpusPath), baseline.corpusSha256, 'corpus'], [readFileSync(manifestPath), baseline.fixtureManifestSha256, 'fixture manifest'],
];
for (const [bytes, expected, label] of bindings) if (sha256(bytes) !== expected) throw new Error(`lowercase-index ${label} digest mismatch`);
for (const [path, expected] of Object.entries(baseline.entrypointSha256)) if (sha256(git('show', `${commit}:${path}`)) !== expected) throw new Error(`lowercase-index TS source mismatch: ${path}`);
const fixturePath = join(repoRoot, manifest.fixture.path);
if (!existsSync(fixturePath) || sha256(readFileSync(fixturePath)) !== manifest.fixture.sha256 || manifest.fixture.sha256 !== baseline.fixtureSha256 || manifest.fixture.pageCount !== 2) throw new Error('lowercase-index fixture identity mismatch');
const ids = corpus.cases.map((entry) => entry.id);
if (ids.length !== 6 || JSON.stringify(Object.keys(oracle.expectations)) !== JSON.stringify(ids)) throw new Error('lowercase-index corpus/oracle IDs differ');
if (Intl.DateTimeFormat().resolvedOptions().locale !== 'en-US' || corpus.localeContract.defaultLocale !== 'en-US' || corpus.localeContract.sentinel !== 'İX' || corpus.localeContract.lowercase !== 'i\u0307x' || corpus.localeContract.sentinel.toLocaleLowerCase() !== corpus.localeContract.lowercase) throw new Error('lowercase-index en-US locale sentinel mismatch');
if (Object.values(corpus.nonclaims).some((value) => value !== false)) throw new Error('lowercase-index nonclaims must remain false');
if (!existsSync(rustCliPath)) throw new Error('missing release Rust CLI');

const observations: Record<string, Json> = {};
const raw: Record<string, Record<string, unknown>> = {};
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
for (const entry of corpus.cases) {
  const { pages, ...options } = entry.input;
  const process = spawnSync(rustCliPath, [], { cwd: repoRoot, encoding: 'utf8', maxBuffer: 100 * 1024 * 1024, input: JSON.stringify({ tool: 'search_pdf', input: { sources: [{ path: join(fixtureDir, entry.fixture), ...(pages !== undefined ? { pages } : {}) }], ...options } }) });
  if (process.status !== 0 || !process.stdout) throw new Error(process.stderr || `${entry.id} subprocess failed`);
  const envelope = JSON.parse(process.stdout) as { status?: string; message?: string; result?: { content: Array<{ text?: string }> } };
  if (envelope.status === 'error') throw new Error(`${entry.id}: ${String(envelope.message)}`);
  const text = envelope.result?.content[0]?.text;
  if (!text) throw new Error(`${entry.id} lacks structured result text`);
  const payload = JSON.parse(text) as { results: Array<Record<string, unknown>> };
  raw[entry.id] = payload.results[0]!;
  const actual = canonicalLowercaseIndexResult(payload.results[0]);
  observations[entry.id] = actual;
  const expected = oracle.expectations[entry.id]!;
  if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
}

const leaves = (value: Json, prefix: Array<string | number> = []): Array<Array<string | number>> => {
  if (Array.isArray(value)) return value.flatMap((entry, index) => leaves(entry, [...prefix, index]));
  if (value && typeof value === 'object') return Object.entries(value).flatMap(([key, entry]) => leaves(entry, [...prefix, key]));
  return [prefix];
};
const mutate = (value: Json, path: Array<string | number>): Json => {
  const result = structuredClone(value); let cursor = result as never;
  for (const segment of path.slice(0, -1)) cursor = cursor[segment as never];
  const key = path.at(-1)!; const original = cursor[key as never] as Json;
  cursor[key as never] = (typeof original === 'string' ? `${original}-mutated` : typeof original === 'number' ? original + 1 : original === null ? 'mutated' : !original) as never;
  return result;
};
let leafMutationCount = 0;
const leafPaths: Record<string, Array<Array<string | number>>> = {};
for (const [id, expectation] of Object.entries(oracle.expectations)) {
  leafPaths[id] = leaves(expectation);
  for (const path of leafPaths[id]!) {
    if (same(observations[id]!, mutate(expectation, path))) throw new Error(`lowercase-index comparator missed ${id}.${path.join('.')}`);
    leafMutationCount += 1;
  }
}

const base = raw['capital-i-dot-query-ci']!;
const warningBase = raw['ascii-whole-word-pages-omitted-bounded']!;
const projectionRejects = (source: Record<string, unknown>, change: (value: Record<string, unknown>) => void, label: string): void => {
  const changed = structuredClone(source); change(changed);
  try { canonicalLowercaseIndexResult(changed); } catch { return; }
  throw new Error(`lowercase-index strict projection accepted ${label}`);
};
const wrongType = [
  [base, (value: Record<string, unknown>) => { (value.searched_pages as unknown[])[0] = '1'; }, 'searched_pages[0]'],
  [base, (value: Record<string, unknown>) => { value.total_matches = '1'; }, 'total_matches'],
  [base, (value: Record<string, unknown>) => { ((value.matches as Array<Record<string, unknown>>)[0]!).text = 7; }, 'match.text'],
  [base, (value: Record<string, unknown>) => { ((value.matches as Array<Record<string, unknown>>)[0]!).match_start = '2'; }, 'match.match_start'],
  [warningBase, (value: Record<string, unknown>) => { (value.warnings as unknown[])[0] = 7; }, 'warning[0]'],
] as const;
for (const [source, change, label] of wrongType) projectionRejects(source, change, label);
const unexpected = [
  [base, (value: Record<string, unknown>) => { value.unexpected = true; }, 'result'],
  [base, (value: Record<string, unknown>) => { ((value.matches as Array<Record<string, unknown>>)[0]!).unexpected = true; }, 'match'],
] as const;
for (const [source, change, label] of unexpected) projectionRejects(source, change, label);
projectionRejects(base, (value) => { delete ((value.matches as Array<Record<string, unknown>>)[0]!).snippet; }, 'match.snippet');
if (Object.hasOwn(base, 'warnings') || Object.hasOwn(base, 'truncated')) throw new Error('lowercase-index optional omission case leaked warnings/truncated');
if (JSON.stringify(wrongType.map((entry) => entry[2])) !== JSON.stringify(LOWERCASE_INDEX_MUTATION_MANIFEST.wrongPrimitiveTypes) || JSON.stringify(unexpected.map((entry) => entry[2])) !== JSON.stringify(LOWERCASE_INDEX_MUTATION_MANIFEST.unexpectedFields) || JSON.stringify(['result.warnings', 'result.truncated', 'match.snippet']) !== JSON.stringify(LOWERCASE_INDEX_MUTATION_MANIFEST.requiredOmissions)) throw new Error('lowercase-index executed probes differ from manifest');

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
const mutationManifestSha256 = sha256(JSON.stringify({ ...LOWERCASE_INDEX_MUTATION_MANIFEST, leafPaths }));
const report = {
  schemaVersion: 1, profile: 'pdf_reader_v3014_lowercase_index_result', candidateSha,
  baselineCommit: commit, baselineTree: baseline.tree, localeContract: corpus.localeContract,
  corpusSha256: sha256(readFileSync(corpusPath)), oracleSha256: sha256(readFileSync(oraclePath)), runnerSha256: sha256(readFileSync(runnerPath)), projectionSha256: sha256(readFileSync(projectionPath)), generatorSha256: sha256(readFileSync(generatorPath)), fixtureManifestSha256: sha256(readFileSync(manifestPath)), fixtureSha256: manifest.fixture.sha256, entrypointSha256: baseline.entrypointSha256,
  caseCount: ids.length, passed: ids.length - failures.length, skipped: 0,
  mutationSensitive: { allClaimedFields: true, manifestVersion: LOWERCASE_INDEX_MUTATION_MANIFEST.version, mutationManifestSha256, leafMutationCount, wrongPrimitiveTypeProbeCount: wrongType.length, unexpectedFieldProbeCount: unexpected.length, requiredOmissionProbeCount: 3 },
  omissionProof: { noWarnings: !Object.hasOwn(base, 'warnings'), noTruncated: !Object.hasOwn(base, 'truncated') },
  nonclaims: corpus.nonclaims, productTruth: { dropInFor3014: false, publishFreeze: true }, pass: failures.length === 0, failures,
};
if (outputPath) writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify(report, null, 2));
if (failures.length > 0) process.exit(1);
console.error(`v3.0.14 lowercase-index differential: PASS (${ids.length}/${ids.length}, zero skipped)`);
