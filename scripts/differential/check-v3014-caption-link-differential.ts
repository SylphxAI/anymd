#!/usr/bin/env bun

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';
  canonicalCaptionLinkResult,
  CAPTION_LINK_MUTATION_MANIFEST,
  type Json,
} from './v3014-caption-link-projection.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-caption-link-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-caption-link-oracle.json');
const manifestPath = join(scriptDir, 'fixtures/v3014-caption-link-fixture.json');
const runnerPath = join(scriptDir, 'v3014-caption-link-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-caption-link-projection.ts');
const generatorPath = join(scriptDir, 'generate-v3014-caption-link-fixture.ts');
const rustCliPath = join(resolveCargoReleaseDir(repoRoot), 'pdf-reader-cli');
const outputFlag = process.argv.indexOf('--output');
const outputPath = outputFlag >= 0 ? process.argv[outputFlag + 1] : undefined;
const sha256 = (value: Uint8Array | string): string => createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => {
  const result = spawnSync('git', args, { cwd: repoRoot });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};
const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as {
  cases: Array<{ id: string; input: Record<string, unknown> }>;
};
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as {
  baseline: Record<string, unknown> & { tag: string; commit: string; tree: string; entrypointSha256: Record<string, string> };
  expectations: Record<string, Json>;
};
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as {
  fixture: { path: string; bytes: number; sha256: string; pageCount: number };
};
const same = (left: Json, right: Json): boolean => JSON.stringify(left) === JSON.stringify(right);
const materialize = (value: unknown): unknown => {
  if (Array.isArray(value)) return value.map(materialize);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value as Record<string, unknown>).map(([key, entry]) => [
      key === 'fixture' ? 'path' : key,
      key === 'fixture' && typeof entry === 'string' ? join(fixtureDir, entry) : materialize(entry),
    ]));
  }
  return value;
};
const baseline = oracle.baseline;
const commit = git('rev-list', '-n', '1', baseline.tag).toString().trim();
if (commit !== baseline.commit) throw new Error('v3.0.14 caption-link baseline tag moved');
const tree = git('rev-parse', `${commit}^{tree}`).toString().trim();
if (tree !== baseline.tree) throw new Error('v3.0.14 caption-link baseline tree mismatch');
const bindings: Array<[Uint8Array, unknown, string]> = [
  [git('show', `${commit}:bun.lock`), baseline.bunLockSha256, 'bun lock'],
  [readFileSync(runnerPath), baseline.runnerSha256, 'runner'],
  [readFileSync(projectionPath), baseline.projectionSha256, 'projection'],
  [readFileSync(generatorPath), baseline.generatorSha256, 'generator'],
  [readFileSync(corpusPath), baseline.corpusSha256, 'corpus'],
  [readFileSync(manifestPath), baseline.fixtureManifestSha256, 'fixture manifest'],
];
for (const [bytes, expected, label] of bindings) {
  if (sha256(bytes) !== expected) throw new Error(`caption-link ${label} digest mismatch`);
}
for (const [path, expected] of Object.entries(baseline.entrypointSha256)) {
  if (sha256(git('show', `${commit}:${path}`)) !== expected) throw new Error(`caption-link TS source mismatch: ${path}`);
}
const fixturePath = join(repoRoot, manifest.fixture.path);
if (!existsSync(fixturePath)) throw new Error('caption-link fixture missing');
const fixture = readFileSync(fixturePath);
if (fixture.length !== manifest.fixture.bytes || sha256(fixture) !== manifest.fixture.sha256 || manifest.fixture.sha256 !== baseline.fixtureSha256) {
  throw new Error('caption-link fixture identity mismatch');
}
const expectedIds = [
  'caption-links-ast-only-hidden-dependencies',
  'caption-links-exposed-dependencies',
  'caption-links-kind-mismatch',
  'caption-links-reverse-ids',
  'caption-links-gap-overlap-boundaries',
  'caption-links-selected-pages-isolation',
];
if (JSON.stringify(corpus.cases.map((entry) => entry.id)) !== JSON.stringify(expectedIds) ||
    JSON.stringify(Object.keys(oracle.expectations).sort()) !== JSON.stringify([...expectedIds].sort())) {
  throw new Error('caption-link corpus/oracle IDs differ');
}
const oracleText = JSON.stringify(oracle.expectations);
for (const token of ['above', 'below', 'left', 'right', 'overlapping', 'caption_ids', 'caption_links', 'caption_link_count']) {
  if (!oracleText.includes(`"${token}"`)) throw new Error(`caption-link oracle lacks ${token}`);
}
if (!existsSync(rustCliPath)) throw new Error('missing release Rust CLI');
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
const observations: Record<string, Json> = {};
const raw: Record<string, Record<string, unknown>> = {};
for (const entry of corpus.cases) {
  const process = spawnSync(rustCliPath, [], {
    cwd: repoRoot,
    encoding: 'utf8',
    input: JSON.stringify({ tool: 'read_pdf', input: materialize(entry.input) }),
    maxBuffer: 100 * 1024 * 1024,
  });
  if (process.status !== 0 || !process.stdout) throw new Error(process.stderr || `${entry.id} failed`);
  const envelope = JSON.parse(process.stdout) as { result: { content: Array<{ text?: string }> } };
  const text = envelope.result.content[0]?.text;
  if (!text) throw new Error(`${entry.id} lacks structured text`);
  const payload = JSON.parse(text) as { results: Array<{ success: boolean; data: Record<string, unknown> }> };
  if (!payload.results[0]?.success) throw new Error(`Rust caption-link case failed: ${entry.id}`);
  raw[entry.id] = payload.results[0].data;
  const actual = canonicalCaptionLinkResult(payload.results[0].data);
  const expected = oracle.expectations[entry.id]!;
  observations[entry.id] = actual;
  if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
}

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
const leafMutationPaths: Record<string, Array<Array<string | number>>> = {};
for (const [caseId, expectation] of Object.entries(oracle.expectations)) {
  const paths = leaves(expectation);
  leafMutationPaths[caseId] = paths;
  for (const path of paths) {
    if (same(observations[caseId]!, mutate(expectation, path))) {
      throw new Error(`caption-link comparator missed ${caseId}.${path.join('.')}`);
    }
    leafMutationCount += 1;
  }
}

const baseRaw = raw['caption-links-ast-only-hidden-dependencies']!;
const exposedRaw = raw['caption-links-exposed-dependencies']!;
const firstCaption = (data: Record<string, unknown>): Record<string, unknown> => {
  const root = (data.document_ast as Record<string, unknown>).root as Record<string, unknown>;
  const queue = [root];
  while (queue.length > 0) {
    const node = queue.shift()!;
    if (node.type === 'caption' && Array.isArray(node.caption_links)) return node;
    queue.push(...((node.children as Array<Record<string, unknown>> | undefined) ?? []));
  }
  throw new Error('caption-link raw probe node missing');
};
const firstTable = (data: Record<string, unknown>): Record<string, unknown> => {
  const root = (data.document_ast as Record<string, unknown>).root as Record<string, unknown>;
  const queue = [root];
  while (queue.length > 0) {
    const node = queue.shift()!;
    if (node.type === 'table' && Array.isArray(node.caption_ids) && node.caption_ids.length > 0) return node;
    queue.push(...((node.children as Array<Record<string, unknown>> | undefined) ?? []));
  }
  throw new Error('caption-link raw reverse-ID probe node missing');
};
const projectionRejects = (source: Record<string, unknown>, change: (value: Record<string, unknown>) => void, label: string) => {
  const changed = structuredClone(source);
  change(changed);
  try { canonicalCaptionLinkResult(changed); } catch { return; }
  throw new Error(`caption-link strict projection accepted ${label}`);
};
const wrongPrimitiveTypeProbes: Array<[Record<string, unknown>, (value: Record<string, unknown>) => void, string]> = [
  [baseRaw, (value) => { (firstCaption(value).caption_links as Array<Record<string, unknown>>)[0]!.node_id = 7; }, 'caption_link.node_id'],
  [baseRaw, (value) => { (firstCaption(value).caption_links as Array<Record<string, unknown>>)[0]!.confidence = '0.9'; }, 'caption_link.confidence'],
  [baseRaw, (value) => { ((firstCaption(value).caption_links as Array<Record<string, unknown>>)[0]!.signals as unknown[])[0] = 7; }, 'caption_link.signals[0]'],
  [baseRaw, (value) => { (firstTable(value).caption_ids as unknown[])[0] = 7; }, 'caption_ids[0]'],
  [baseRaw, (value) => { (((value.document_ast as Record<string, unknown>).summary as Record<string, unknown>).caption_link_count) = '1'; }, 'summary.caption_link_count'],
];
const unexpectedFieldProbes: Array<[Record<string, unknown>, (value: Record<string, unknown>) => void, string]> = [
  [baseRaw, (value) => { (value.document_ast as Record<string, unknown>).unexpected = true; }, 'document_ast'],
  [baseRaw, (value) => { firstCaption(value).unexpected = true; }, 'node'],
  [baseRaw, (value) => { (firstCaption(value).caption_links as Array<Record<string, unknown>>)[0]!.unexpected = true; }, 'caption_link'],
  [exposedRaw, (value) => { (value.elements as Array<Record<string, unknown>>)[0]!.unexpected = true; }, 'element'],
];
const requiredOmissionProbes: Array<[Record<string, unknown>, (value: Record<string, unknown>) => void, string]> = [
  [baseRaw, (value) => { delete value.document_ast; }, 'document_ast'],
  [baseRaw, (value) => { delete (value.document_ast as Record<string, unknown>).root; }, 'root'],
  [baseRaw, (value) => { delete ((value.document_ast as Record<string, unknown>).summary as Record<string, unknown>).caption_link_count; }, 'summary.caption_link_count'],
  [baseRaw, (value) => { delete (firstCaption(value).caption_links as Array<Record<string, unknown>>)[0]!.node_id; }, 'caption_link.node_id'],
  [baseRaw, (value) => { delete (firstCaption(value).caption_links as Array<Record<string, unknown>>)[0]!.signals; }, 'caption_link.signals'],
];
const assertProbeManifest = (actual: string[], expected: readonly string[], label: string): void => {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`caption-link ${label} probes differ from the declared manifest`);
  }
};
assertProbeManifest(wrongPrimitiveTypeProbes.map(([, , id]) => id), CAPTION_LINK_MUTATION_MANIFEST.wrongPrimitiveTypes, 'wrong-type');
assertProbeManifest(unexpectedFieldProbes.map(([, , id]) => id), CAPTION_LINK_MUTATION_MANIFEST.unexpectedFields, 'unexpected-field');
assertProbeManifest(requiredOmissionProbes.map(([, , id]) => id), CAPTION_LINK_MUTATION_MANIFEST.requiredOmissions, 'required-omission');
for (const [source, change, label] of wrongPrimitiveTypeProbes) projectionRejects(source, change, label);
for (const [source, change, label] of unexpectedFieldProbes) projectionRejects(source, change, label);
for (const [source, change, label] of requiredOmissionProbes) projectionRejects(source, change, label);
let privateLeakProbeCount = 0;
for (const key of CAPTION_LINK_MUTATION_MANIFEST.privateLeakage) {
  const changed = structuredClone(baseRaw);
  changed[key] = { leaked: true };
  if (same(canonicalCaptionLinkResult(changed), oracle.expectations[expectedIds[0]]!)) throw new Error(`caption-link private leak undetected: ${key}`);
  privateLeakProbeCount += 1;
}
let dependencyPresenceProbeCount = 0;
for (const key of CAPTION_LINK_MUTATION_MANIFEST.dependencyPresence) {
  for (const id of expectedIds) {
    const expected = (oracle.expectations[id] as Record<string, Json>).dependency_surfaces as Record<string, Json>;
    if (Object.hasOwn(raw[id]!, key) !== expected[key]) throw new Error(`caption-link dependency presence mismatch: ${id}.${key}`);
  }
  dependencyPresenceProbeCount += 1;
}
const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
const executedMutationManifest = {
  ...CAPTION_LINK_MUTATION_MANIFEST,
  leafPaths: leafMutationPaths,
};
const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_caption_link_result',
  candidateSha,
  baselineCommit: commit,
  baselineTree: tree,
  corpusSha256: sha256(readFileSync(corpusPath)),
  oracleSha256: sha256(readFileSync(oraclePath)),
  fixtureSha256: manifest.fixture.sha256,
  caseCount: corpus.cases.length,
  passed: corpus.cases.length - failures.length,
  skipped: 0,
  mutationSensitive: {
    allClaimedFields: true,
    manifestVersion: CAPTION_LINK_MUTATION_MANIFEST.version,
    mutationManifestSha256: sha256(JSON.stringify(executedMutationManifest)),
    leafMutationCount,
    wrongPrimitiveTypeProbeCount: wrongPrimitiveTypeProbes.length,
    unexpectedFieldProbeCount: unexpectedFieldProbes.length,
    requiredOmissionProbeCount: requiredOmissionProbes.length,
    privateLeakProbeCount,
    dependencyPresenceProbeCount,
  },
  pass: failures.length === 0,
  observations,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
