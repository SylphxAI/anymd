#!/usr/bin/env bun

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalSearchSemanticFailure, canonicalSearchSemanticResult, SEARCH_SEMANTIC_MUTATION_MANIFEST, type Json } from './v3014-search-semantic-projection.ts';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-search-semantic-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-search-semantic-oracle.json');
const manifestPath = join(scriptDir, 'fixtures/v3014-search-semantic-fixture.json');
const runnerPath = join(scriptDir, 'v3014-search-semantic-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-search-semantic-projection.ts');
const generatorPath = join(scriptDir, 'generate-v3014-search-semantic-fixture.ts');
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
  nonclaim: { utf16SplitSurrogateWireParity: boolean; reason: string; excludedProbe: unknown };
  cases: Array<{ id: string; fixture: string; input: Record<string, unknown> }>;
};
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as {
  baseline: Record<string, unknown> & { tag: string; commit: string; tree: string; entrypointSha256: Record<string, string> };
  expectations: Record<string, Json>;
};
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as {
  fixture: { path: string; bytes: number; sha256: string; pageCount: number };
  reusedFixture: { path: string; sha256: string };
};
const same = (left: Json, right: Json): boolean => JSON.stringify(left) === JSON.stringify(right);
const baseline = oracle.baseline;
const commit = git('rev-list', '-n', '1', baseline.tag).toString().trim();
if (commit !== baseline.commit || commit !== '92651c79c6ce8d10dfa3c76332176c26f222bd78') throw new Error('v3.0.14 search-semantic baseline tag moved');
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== baseline.tree) throw new Error('v3.0.14 search-semantic baseline tree mismatch');
const bindings: Array<[Uint8Array, unknown, string]> = [
  [git('show', `${commit}:bun.lock`), baseline.bunLockSha256, 'bun lock'],
  [readFileSync(runnerPath), baseline.runnerSha256, 'runner'],
  [readFileSync(projectionPath), baseline.projectionSha256, 'projection'],
  [readFileSync(generatorPath), baseline.generatorSha256, 'generator'],
  [readFileSync(corpusPath), baseline.corpusSha256, 'corpus'],
  [readFileSync(manifestPath), baseline.fixtureManifestSha256, 'fixture manifest'],
];
for (const [bytes, expected, label] of bindings) if (sha256(bytes) !== expected) throw new Error(`search-semantic ${label} digest mismatch`);
for (const [path, expected] of Object.entries(baseline.entrypointSha256)) {
  if (sha256(git('show', `${commit}:${path}`)) !== expected) throw new Error(`search-semantic TS source mismatch: ${path}`);
}
for (const fixture of [manifest.fixture, manifest.reusedFixture]) {
  const path = join(repoRoot, fixture.path);
  if (!existsSync(path) || sha256(readFileSync(path)) !== fixture.sha256) throw new Error(`search-semantic fixture identity mismatch: ${fixture.path}`);
}
if (manifest.fixture.sha256 !== baseline.fixtureSha256 || manifest.reusedFixture.sha256 !== baseline.reusedFixtureSha256 || manifest.fixture.pageCount !== 3) throw new Error('search-semantic fixture baseline mismatch');
const ids = corpus.cases.map((entry) => entry.id);
if (ids.length !== 12 || JSON.stringify(Object.keys(oracle.expectations)) !== JSON.stringify(ids)) throw new Error('search-semantic corpus/oracle IDs differ');
if (corpus.nonclaim.utf16SplitSurrogateWireParity !== false || !corpus.nonclaim.reason.includes('cannot represent a lone UTF-16 surrogate')) throw new Error('search-semantic split-surrogate nonclaim missing');
if (!existsSync(rustCliPath)) throw new Error('missing release Rust CLI');

const observations: Record<string, Json> = {};
const raw: Record<string, Record<string, unknown>> = {};
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
for (const entry of corpus.cases) {
  const { pages, ...options } = entry.input;
  const fixturePath = join(fixtureDir, entry.fixture);
  const process = spawnSync(rustCliPath, [], {
    cwd: repoRoot, encoding: 'utf8', maxBuffer: 100 * 1024 * 1024,
    input: JSON.stringify({ tool: 'search_pdf', input: { sources: [{ path: fixturePath, ...(pages !== undefined ? { pages } : {}) }], ...options } }),
  });
  if (process.status !== 0 || !process.stdout) throw new Error(process.stderr || `${entry.id} subprocess failed`);
  const envelope = JSON.parse(process.stdout) as Record<string, unknown>;
  let actual: Json;
  if (envelope.status === 'error') {
    actual = canonicalSearchSemanticFailure(String(envelope.message).replaceAll(fixtureDir, '<fixture-dir>'));
  } else {
    const result = envelope.result as { content: Array<{ text?: string }> };
    const text = result.content[0]?.text;
    if (!text) throw new Error(`${entry.id} lacks structured result text`);
    const payload = JSON.parse(text) as { results: Array<Record<string, unknown>> };
    raw[entry.id] = payload.results[0]!;
    actual = canonicalSearchSemanticResult(payload.results[0]);
  }
  observations[entry.id] = actual;
  const expected = oracle.expectations[entry.id]!;
  if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
}

const excludedProbe = corpus.nonclaim.excludedProbe as { text: string; query: string; context_chars: number };
if (excludedProbe.text !== 'AA😀résuméZZ' || excludedProbe.query !== 'résumé' || excludedProbe.context_chars !== 1) {
  throw new Error('split-surrogate excluded probe contract drifted');
}
const excludedProcess = spawnSync(rustCliPath, [], {
  cwd: repoRoot, encoding: 'utf8', maxBuffer: 100 * 1024 * 1024,
  input: JSON.stringify({
    tool: 'search_pdf',
    input: {
      sources: [{ path: join(fixtureDir, 'v3014-search-semantic-v1.pdf'), pages: [2] }],
      query: excludedProbe.query,
      context_chars: excludedProbe.context_chars,
      case_sensitive: true,
      max_pages: 10,
      max_matches_per_source: 50,
    },
  }),
});
if (excludedProcess.status !== 0 || !excludedProcess.stdout) throw new Error('split-surrogate fail-closed probe subprocess failed');
const excludedEnvelope = JSON.parse(excludedProcess.stdout) as Record<string, unknown>;
const splitSurrogateFailClosed = excludedEnvelope.status === 'error' &&
  String(excludedEnvelope.message ?? '').includes('UTF-16 snippet context would split an astral character');
if (!splitSurrogateFailClosed) throw new Error('split-surrogate excluded probe did not fail closed');

const leaves = (value: Json, prefix: Array<string | number> = []): Array<Array<string | number>> => {
  if (Array.isArray(value)) return value.flatMap((entry, index) => leaves(entry, [...prefix, index]));
  if (value && typeof value === 'object') return Object.entries(value).flatMap(([key, entry]) => leaves(entry, [...prefix, key]));
  return [prefix];
};
const mutate = (value: Json, path: Array<string | number>): Json => {
  const result = structuredClone(value);
  let cursor = result as never;
  for (const segment of path.slice(0, -1)) cursor = cursor[segment as never];
  const key = path.at(-1)!;
  const original = cursor[key as never] as Json;
  cursor[key as never] = (typeof original === 'string' ? `${original}-mutated` : typeof original === 'number' ? original + 1 : original === null ? 'mutated' : !original) as never;
  return result;
};
let leafMutationCount = 0;
const leafPaths: Record<string, Array<Array<string | number>>> = {};
for (const [id, expectation] of Object.entries(oracle.expectations)) {
  leafPaths[id] = leaves(expectation);
  for (const path of leafPaths[id]!) {
    if (same(observations[id]!, mutate(expectation, path))) throw new Error(`search-semantic comparator missed ${id}.${path.join('.')}`);
    leafMutationCount += 1;
  }
}

const base = raw['invalid-then-truncated-warning-order']!;
const noWarning = raw['no-warning-and-truncated-omission']!;
const projectionRejects = (source: Record<string, unknown>, change: (value: Record<string, unknown>) => void, label: string): void => {
  const changed = structuredClone(source); change(changed);
  try { canonicalSearchSemanticResult(changed); } catch { return; }
  throw new Error(`search-semantic strict projection accepted ${label}`);
};
const wrongType = [
  [base, (value: Record<string, unknown>) => { (value.searched_pages as unknown[])[0] = '1'; }, 'searched_pages[0]'],
  [base, (value: Record<string, unknown>) => { ((value.matches as Array<Record<string, unknown>>)[0]!).snippet = 7; }, 'match.snippet'],
  [base, (value: Record<string, unknown>) => { (value.warnings as unknown[])[0] = 7; }, 'warning[0]'],
] as const;
for (const [source, change, label] of wrongType) projectionRejects(source, change, label);
try { canonicalSearchSemanticFailure(7); throw new Error('search-semantic failure type accepted'); } catch (error) { if (error instanceof Error && error.message === 'search-semantic failure type accepted') throw error; }
const unexpected = [
  [base, (value: Record<string, unknown>) => { value.unexpected = true; }, 'result'],
  [base, (value: Record<string, unknown>) => { ((value.matches as Array<Record<string, unknown>>)[0]!).unexpected = true; }, 'match'],
] as const;
for (const [source, change, label] of unexpected) projectionRejects(source, change, label);
const omissions = [
  [base, (value: Record<string, unknown>) => { delete value.success; }, 'result.success'],
  [base, (value: Record<string, unknown>) => { delete ((value.matches as Array<Record<string, unknown>>)[0]!).snippet; }, 'match.snippet'],
] as const;
for (const [source, change, label] of omissions) projectionRejects(source, change, label);
if (Object.hasOwn(noWarning, 'warnings') || Object.hasOwn(noWarning, 'truncated')) throw new Error('search-semantic omission case leaked warnings/truncated');
const declared = SEARCH_SEMANTIC_MUTATION_MANIFEST;
if (JSON.stringify([...wrongType.map((entry) => entry[2]), 'failure.error']) !== JSON.stringify(declared.wrongPrimitiveTypes) || JSON.stringify(unexpected.map((entry) => entry[2])) !== JSON.stringify(declared.unexpectedFields) || JSON.stringify(omissions.map((entry) => entry[2])) !== JSON.stringify(declared.requiredOmissions)) throw new Error('search-semantic executed probes differ from manifest');

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
const mutationManifestSha256 = sha256(JSON.stringify({ ...declared, leafPaths }));
const report = {
  schemaVersion: 1, profile: 'pdf_reader_v3014_search_semantic_result', candidateSha,
  baselineCommit: commit, baselineTree: baseline.tree,
  corpusSha256: sha256(readFileSync(corpusPath)), oracleSha256: sha256(readFileSync(oraclePath)),
  runnerSha256: sha256(readFileSync(runnerPath)), projectionSha256: sha256(readFileSync(projectionPath)),
  generatorSha256: sha256(readFileSync(generatorPath)), fixtureManifestSha256: sha256(readFileSync(manifestPath)),
  fixtureSha256: manifest.fixture.sha256, reusedFixtureSha256: manifest.reusedFixture.sha256,
  entrypointSha256: baseline.entrypointSha256,
  caseCount: ids.length, passed: ids.length - failures.length, skipped: 0,
  mutationSensitive: { allClaimedFields: true, manifestVersion: declared.version, mutationManifestSha256, leafMutationCount, wrongPrimitiveTypeProbeCount: 4, unexpectedFieldProbeCount: unexpected.length, requiredOmissionProbeCount: omissions.length },
  omissionProof: { noWarnings: !Object.hasOwn(noWarning, 'warnings'), noTruncated: !Object.hasOwn(noWarning, 'truncated') },
  nonclaim: { ...corpus.nonclaim, failClosedProof: splitSurrogateFailClosed },
  productTruth: { dropInFor3014: false, publishFreeze: true },
  pass: failures.length === 0,
  failures,
};
if (outputPath) writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
console.log(JSON.stringify(report, null, 2));
if (failures.length > 0) process.exit(1);
console.error(`v3.0.14 search-semantic differential: PASS (${ids.length}/${ids.length}, zero skipped)`);
