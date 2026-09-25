#!/usr/bin/env bun

import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalVisualCandidateResult, VISUAL_CANDIDATE_MUTATION_MANIFEST, type Json } from './v3014-visual-candidate-projection.ts';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-visual-candidate-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-visual-candidate-oracle.json');
const manifestPath = join(scriptDir, 'fixtures/v3014-visual-candidate-fixtures.json');
const runnerPath = join(scriptDir, 'v3014-visual-candidate-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-visual-candidate-projection.ts');
const generatorPath = join(scriptDir, 'generate-v3014-visual-candidate-fixtures.ts');
const rustServerPath = join(resolveCargoReleaseDir(repoRoot), 'anymd');
const outputIndex = process.argv.indexOf('--output');
const outputPath = outputIndex >= 0 ? process.argv[outputIndex + 1] : undefined;
const sha256 = (value: Uint8Array | string): string => createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => { const result = spawnSync('git', args, { cwd: repoRoot }); if (result.status !== 0) throw new Error(result.stderr.toString()); return result.stdout; };
const same = (left: Json, right: Json): boolean => JSON.stringify(left) === JSON.stringify(right);
type Case = { id: string; fixture: string; input: Record<string, unknown> };
type Corpus = { envelope: { fixtureCount: number; caseCount: number; maxPagesPerCase: number; maxCandidatesPerCase: number }; nonclaims: Record<string, boolean>; cases: Case[] };
type Oracle = { baseline: { tag: string; commit: string; tree: string; bunLockSha256: string; runnerSha256: string; projectionSha256: string; generatorSha256: string; corpusSha256: string; fixtureManifestSha256: string; fixtureSha256: Record<string, string>; envelope: Corpus['envelope']; nonclaims: Record<string, boolean>; entrypointSha256: Record<string, string> }; expectations: Record<string, Json> };
const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as Corpus;
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as { fixtures: Array<{ path: string; sha256: string }> };

const commit = git('rev-list', '-n', '1', oracle.baseline.tag).toString().trim();
if (commit !== oracle.baseline.commit || commit !== '92651c79c6ce8d10dfa3c76332176c26f222bd78') throw new Error('v3.0.14 visual-candidate tag moved');
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== oracle.baseline.tree) throw new Error('v3.0.14 visual-candidate tree mismatch');
const bindings: Array<[Uint8Array, string, string]> = [[git('show', `${commit}:bun.lock`), oracle.baseline.bunLockSha256, 'bun lock'], [readFileSync(runnerPath), oracle.baseline.runnerSha256, 'runner'], [readFileSync(projectionPath), oracle.baseline.projectionSha256, 'projection'], [readFileSync(generatorPath), oracle.baseline.generatorSha256, 'generator'], [readFileSync(corpusPath), oracle.baseline.corpusSha256, 'corpus'], [readFileSync(manifestPath), oracle.baseline.fixtureManifestSha256, 'fixture manifest']];
for (const [bytes, digest, label] of bindings) if (sha256(bytes) !== digest) throw new Error(`visual-candidate ${label} digest mismatch`);
for (const [path, digest] of Object.entries(oracle.baseline.entrypointSha256)) if (sha256(git('show', `${commit}:${path}`)) !== digest) throw new Error(`visual-candidate TS source mismatch: ${path}`);
if (manifest.fixtures.length !== 1 || corpus.envelope.fixtureCount !== 1 || corpus.envelope.caseCount !== 11 || corpus.envelope.maxPagesPerCase !== 2 || corpus.envelope.maxCandidatesPerCase !== 2) throw new Error('visual-candidate envelope changed');
if (JSON.stringify(corpus.envelope) !== JSON.stringify(oracle.baseline.envelope) || JSON.stringify(corpus.nonclaims) !== JSON.stringify(oracle.baseline.nonclaims)) throw new Error('visual-candidate envelope/nonclaims differ from oracle');
if (corpus.nonclaims.dropInFor3014 !== false || corpus.nonclaims.providerExecution !== false) throw new Error('visual-candidate nonclaims weakened');
for (const fixture of manifest.fixtures) { const path = join(repoRoot, fixture.path); if (!existsSync(path) || sha256(readFileSync(path)) !== fixture.sha256 || oracle.baseline.fixtureSha256[fixture.path] !== fixture.sha256) throw new Error(`visual-candidate fixture mismatch: ${fixture.path}`); }
const ids = corpus.cases.map((entry) => entry.id);
if (new Set(ids).size !== 11 || JSON.stringify(Object.keys(oracle.expectations)) !== JSON.stringify(ids)) throw new Error('visual-candidate corpus/oracle IDs differ');
if (!existsSync(rustServerPath)) throw new Error('missing release Rust MCP server');

const observations: Record<string, Json> = {};
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
const child = spawn(rustServerPath, [], { cwd: repoRoot, stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, MCP_TRANSPORT: 'stdio', MCP_PDF_REGION_ANALYSIS_COMMAND: '', MCP_PDF_REGION_ANALYSIS_HTTP_URL: '', MCP_PDF_REGION_ANALYSIS_PRESET: '' } }) as ChildProcessWithoutNullStreams;
let buffer = ''; let stderr = ''; const pending = new Map<number, (value: Record<string, unknown>) => void>();
child.stderr.on('data', (chunk: Buffer) => { stderr += chunk.toString(); });
child.stdout.on('data', (chunk: Buffer) => { buffer += chunk.toString(); while (buffer.includes('\n')) { const index = buffer.indexOf('\n'); const line = buffer.slice(0, index).trim(); buffer = buffer.slice(index + 1); if (!line) continue; const response = JSON.parse(line) as Record<string, unknown>; const id = Number(response.id); pending.get(id)?.(response); pending.delete(id); } });
const request = (id: number, method: string, params: unknown): Promise<Record<string, unknown>> => new Promise((resolve, reject) => { const timer = setTimeout(() => { pending.delete(id); reject(new Error(`Rust visual-candidate request timed out: ${stderr.slice(-2000)}`)); }, 60_000); pending.set(id, (value) => { clearTimeout(timer); resolve(value); }); child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`); });
try {
  await request(1, 'initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'v3014-visual-candidate-differential', version: '1' } });
  child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
  for (const [index, entry] of corpus.cases.entries()) {
    const input = structuredClone(entry.input); const source = (input.sources as Array<Record<string, unknown>>)[0]; if (!source) throw new Error(`${entry.id} lacks source`); source.path = join(fixtureDir, entry.fixture);
    const response = await request(index + 10, 'tools/call', { name: 'read_pdf', arguments: input });
    if (response.error) throw new Error(`${entry.id}: ${JSON.stringify(response.error)}`);
    const actual = canonicalVisualCandidateResult(response.result); observations[entry.id] = actual; const expected = oracle.expectations[entry.id]!; if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
  }
} finally { child.kill('SIGTERM'); }

const leaves = (value: Json, prefix: Array<string | number> = []): Array<Array<string | number>> => Array.isArray(value) ? value.flatMap((entry, index) => leaves(entry, [...prefix, index])) : value && typeof value === 'object' ? Object.entries(value).flatMap(([key, entry]) => leaves(entry, [...prefix, key])) : [prefix];
const mutate = (value: Json, path: Array<string | number>): Json => { const changed = structuredClone(value); let cursor = changed as never; for (const segment of path.slice(0, -1)) cursor = cursor[segment as never]; const key = path.at(-1)!; const original = cursor[key as never] as Json; cursor[key as never] = (typeof original === 'string' ? `${original}-mutated` : typeof original === 'number' ? original + 1 : original === null ? 'mutated' : !original) as never; return changed; };
let leafMutationCount = 0; const leafPaths: Record<string, Array<Array<string | number>>> = {};
for (const [id, expectation] of Object.entries(oracle.expectations)) { leafPaths[id] = leaves(expectation); for (const path of leafPaths[id]!) { if (same(expectation, mutate(expectation, path))) throw new Error(`visual-candidate comparator missed ${id}.${path.join('.')}`); leafMutationCount += 1; } }
const directExpected = oracle.expectations['direct-table-target'] as Record<string, Json>;
const directCandidate = structuredClone((directExpected.visual_enrichment_candidates as Json[])[0]!);
const mapExpected = oracle.expectations['document-map-candidate-routing'] as Record<string, Json>;
const strictBase: Record<string, unknown> = { num_pages: 7, visual_enrichment_candidates: [directCandidate] };
const strictMapBase: Record<string, unknown> = {
  num_pages: 7,
  visual_enrichment_candidates: structuredClone(mapExpected.visual_enrichment_candidates),
  document_map: structuredClone(mapExpected.document_map),
};
const assertRejects = (source: Record<string, unknown>, change: (value: Record<string, unknown>) => void, label: string): void => {
  const changed = structuredClone(source); change(changed);
  try { canonicalVisualCandidateResult(changed); } catch { return; }
  throw new Error(`visual-candidate strict projection accepted ${label}`);
};
const firstCandidate = (value: Record<string, unknown>): Record<string, unknown> => (value.visual_enrichment_candidates as Array<Record<string, unknown>>)[0]!;
const candidateRegion = (value: Record<string, unknown>): Record<string, unknown> => firstCandidate(value).region as Record<string, unknown>;
const candidateBox = (value: Record<string, unknown>): Record<string, unknown> => candidateRegion(value).bounding_box as Record<string, unknown>;
const map = (value: Record<string, unknown>): Record<string, unknown> => value.document_map as Record<string, unknown>;
const mapPage = (value: Record<string, unknown>): Record<string, unknown> => (map(value).pages as Array<Record<string, unknown>>)[0]!;
const mapSummary = (value: Record<string, unknown>): Record<string, unknown> => map(value).summary as Record<string, unknown>;
assertRejects(strictBase, (value) => { firstCandidate(value).page = '1'; }, 'candidate.page wrong type');
assertRejects(strictBase, (value) => { firstCandidate(value).target_element_type = 1; }, 'candidate.target_element_type wrong type');
assertRejects(strictBase, (value) => { candidateBox(value).left = '72'; }, 'candidate bounding-box coordinate wrong type');
assertRejects(strictMapBase, (value) => { mapPage(value).visual_candidate_count = '1'; }, 'Document Map candidate count wrong type');
assertRejects(strictBase, (value) => { firstCandidate(value).unexpected = true; }, 'unexpected candidate field');
assertRejects(strictBase, (value) => { candidateRegion(value).unexpected = true; }, 'unexpected region field');
assertRejects(strictBase, (value) => { delete firstCandidate(value).id; }, 'required candidate id omission');
assertRejects(strictBase, (value) => { delete candidateRegion(value).bounding_box; }, 'required candidate bounding box omission');
assertRejects(strictMapBase, (value) => { delete mapPage(value).visual_candidate_count; }, 'required Document Map page count omission');
assertRejects(strictMapBase, (value) => { delete mapSummary(value).visual_enrichment_candidate_count; }, 'required Document Map summary count omission');
const strictExpected = canonicalVisualCandidateResult(strictBase);
let privateLeakProbeCount = 0;
for (const key of VISUAL_CANDIDATE_MUTATION_MANIFEST.privateLeakage) {
  const changed = structuredClone(strictBase); changed[key] = { leaked: true };
  if (same(canonicalVisualCandidateResult(changed), strictExpected)) throw new Error(`visual-candidate private leakage undetected: ${key}`);
  privateLeakProbeCount += 1;
}
let dependencyPresenceProbeCount = 0;
for (const key of VISUAL_CANDIDATE_MUTATION_MANIFEST.dependencyPresence) {
  const changed = structuredClone(strictBase);
  if (Object.hasOwn(changed, key)) delete changed[key]; else changed[key] = [];
  try {
    if (same(canonicalVisualCandidateResult(changed), strictExpected)) throw new Error(`visual-candidate dependency presence undetected: ${key}`);
  } catch (error: unknown) {
    if (error instanceof Error && error.message.startsWith('visual-candidate dependency presence undetected:')) throw error;
  }
  dependencyPresenceProbeCount += 1;
}
const provider = oracle.expectations['provider-not-configured-retains-candidate'] as Record<string, Json>;
const hidden = oracle.expectations['internal-elements-hidden'] as Record<string, Json>;
const control = oracle.expectations['false-control'] as Record<string, Json>;
if (provider.has_visual_enrichments !== false || provider.visual_enrichments?.length !== 0 || provider.visual_enrichment_candidates?.length !== 1) throw new Error('provider-not-configured omission proof weakened');
if (hidden.has_elements !== false || hidden.visual_enrichment_candidates?.length !== 1) throw new Error('internal element visibility proof weakened');
if (control.has_visual_enrichment_candidates !== false || control.visual_enrichment_candidates?.length !== 0) throw new Error('false-control proof weakened');
const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
const report = { schemaVersion: 1, profile: 'pdf_reader_v3014_visual_candidate_result', candidateSha, baselineCommit: commit, baselineTree: oracle.baseline.tree, corpusSha256: sha256(readFileSync(corpusPath)), oracleSha256: sha256(readFileSync(oraclePath)), runnerSha256: sha256(readFileSync(runnerPath)), projectionSha256: sha256(readFileSync(projectionPath)), generatorSha256: sha256(readFileSync(generatorPath)), fixtureManifestSha256: sha256(readFileSync(manifestPath)), fixtureSha256: oracle.baseline.fixtureSha256, entrypointSha256: oracle.baseline.entrypointSha256, envelope: corpus.envelope, caseCount: ids.length, passed: ids.length - failures.length, skipped: 0, mutationSensitive: { allClaimedFields: true, manifestVersion: VISUAL_CANDIDATE_MUTATION_MANIFEST.version, mutationManifestSha256: sha256(JSON.stringify({ ...VISUAL_CANDIDATE_MUTATION_MANIFEST, leafPaths })), leafMutationCount, wrongPrimitiveTypeProbeCount: VISUAL_CANDIDATE_MUTATION_MANIFEST.wrongPrimitiveTypes.length, unexpectedFieldProbeCount: VISUAL_CANDIDATE_MUTATION_MANIFEST.unexpectedFields.length, requiredOmissionProbeCount: VISUAL_CANDIDATE_MUTATION_MANIFEST.requiredOmissions.length, publicOmissionProbeCount: VISUAL_CANDIDATE_MUTATION_MANIFEST.publicOmissions.length, privateLeakProbeCount, dependencyPresenceProbeCount }, providerIndependentProof: { providerNotConfigured: true, candidatesRetained: true, enrichmentsOmitted: true, internalElementsHidden: true }, nonclaims: corpus.nonclaims, productTruth: { dropInFor3014: false, publishFreeze: true }, pass: failures.length === 0, failures };
const serialized = `${JSON.stringify(report, null, 2)}\n`; if (outputPath) await Bun.write(outputPath, serialized); console.log(serialized.trimEnd()); if (failures.length > 0) process.exit(1); console.error(`v3.0.14 visual-candidate differential: PASS (${String(ids.length)}/${String(ids.length)}, zero skipped)`);
