#!/usr/bin/env bun

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-semantic-hint-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-semantic-hint-oracle.json');
const fixtureManifestPath = join(scriptDir, 'fixtures/v3014-semantic-hint-fixture.json');
const citationFixtureManifestPath = join(
  scriptDir,
  'fixtures/v3014-citation-chunk-fixture.json'
);
const runnerPath = join(scriptDir, 'v3014-semantic-hint-baseline-runner.ts');
const generatorPath = join(scriptDir, 'generate-v3014-semantic-hint-fixture.ts');
const rustCliPath = join(resolveCargoReleaseDir(repoRoot), 'pdf-reader-cli');
const outputFlag = process.argv.indexOf('--output');
const outputPath = outputFlag >= 0 ? process.argv[outputFlag + 1] : undefined;

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
type Case = { id: string; input: Record<string, unknown> };
type Oracle = {
  baseline: {
    tag: string;
    commit: string;
    tree: string;
    bunLockSha256: string;
    runnerSha256: string;
    generatorSha256: string;
    corpusSha256: string;
    fixtureManifestSha256: string;
    fixtureSha256: string;
    citationFixtureManifestSha256: string;
    citationFixtureSha256: string;
    entrypointSha256: Record<string, string>;
  };
  expectations: Record<string, Json>;
};

const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as { cases: Case[] };
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const fixtureManifest = JSON.parse(readFileSync(fixtureManifestPath, 'utf8')) as {
  fixture: { path: string; bytes: number; sha256: string };
};
const citationFixtureManifest = JSON.parse(
  readFileSync(citationFixtureManifestPath, 'utf8')
) as { fixture: { path: string; bytes: number; sha256: string } };
const sha256 = (value: Uint8Array | string): string =>
  createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => {
  const result = spawnSync('git', args, { cwd: repoRoot });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};
const canonHint = (value: unknown): Json => {
  if (!value || typeof value !== 'object') return null;
  const hint = value as Record<string, unknown>;
  return {
    role: String(hint.role),
    confidence: Number(hint.confidence),
    signals: (hint.signals ?? []) as Json,
    ...(hint.level === undefined ? {} : { level: Number(hint.level) }),
  };
};
const canonical = (data: Record<string, unknown>): Json => ({
  elements: ((data.elements ?? []) as Array<Record<string, unknown>>)
    .filter((element) => element.type === 'text')
    .map((element) => ({
      id: String(element.id),
      page: Number(element.page),
      content: String(element.content),
      semantic_hint: canonHint(element.semantic_hint),
    })),
  chunks: ((data.chunks ?? []) as Array<Record<string, unknown>>).map((chunk) => ({
    id: String(chunk.id),
    page_start: Number(chunk.page_start),
    page_end: Number(chunk.page_end),
    text: String(chunk.text),
    element_ids: (chunk.element_ids ?? []) as Json,
    ...(chunk.strategy === undefined ? {} : { strategy: String(chunk.strategy) }),
    ...(chunk.heading === undefined ? {} : { heading: String(chunk.heading) }),
  })),
});
const canonicalJson = (value: Json): Json => {
  if (Array.isArray(value)) return value.map(canonicalJson);
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, entry]) => [key, canonicalJson(entry)])
    );
  }
  return value;
};
const same = (left: Json, right: Json): boolean =>
  JSON.stringify(canonicalJson(left)) === JSON.stringify(canonicalJson(right));

function verifyFixture(
  manifest: { path: string; bytes: number; sha256: string },
  label: string
): void {
  const path = join(repoRoot, manifest.path);
  if (!existsSync(path)) throw new Error(`missing ${label} fixture`);
  const bytes = readFileSync(path);
  if (bytes.length !== manifest.bytes || sha256(bytes) !== manifest.sha256) {
    throw new Error(`${label} fixture digest mismatch`);
  }
}

function verifyAuthority(): Record<string, string> {
  const commit = git('rev-list', '-n', '1', oracle.baseline.tag).toString().trim();
  if (commit !== oracle.baseline.commit) throw new Error('baseline tag moved');
  const tree = git('rev-parse', `${commit}^{tree}`).toString().trim();
  if (tree !== oracle.baseline.tree) throw new Error('baseline tree mismatch');
  if (sha256(git('show', `${commit}:bun.lock`)) !== oracle.baseline.bunLockSha256) {
    throw new Error('baseline lock mismatch');
  }
  if (sha256(readFileSync(runnerPath)) !== oracle.baseline.runnerSha256) {
    throw new Error('baseline runner mismatch');
  }
  if (sha256(readFileSync(generatorPath)) !== oracle.baseline.generatorSha256) {
    throw new Error('semantic-hint generator mismatch');
  }
  if (sha256(readFileSync(corpusPath)) !== oracle.baseline.corpusSha256) {
    throw new Error('semantic-hint corpus mismatch');
  }
  if (sha256(readFileSync(fixtureManifestPath)) !== oracle.baseline.fixtureManifestSha256) {
    throw new Error('semantic-hint fixture manifest mismatch');
  }
  for (const [sourceFile, expected] of Object.entries(oracle.baseline.entrypointSha256)) {
    if (sha256(git('show', `${commit}:${sourceFile}`)) !== expected) {
      throw new Error(`baseline entrypoint mismatch: ${sourceFile}`);
    }
  }
  verifyFixture(fixtureManifest.fixture, 'semantic-hint');
  if (fixtureManifest.fixture.sha256 !== oracle.baseline.fixtureSha256) {
    throw new Error('semantic-hint fixture oracle binding mismatch');
  }
  verifyFixture(citationFixtureManifest.fixture, 'citation-chunk');
  if (
    sha256(readFileSync(citationFixtureManifestPath)) !==
    oracle.baseline.citationFixtureManifestSha256
  ) {
    throw new Error('citation-chunk fixture manifest oracle binding mismatch');
  }
  if (citationFixtureManifest.fixture.sha256 !== oracle.baseline.citationFixtureSha256) {
    throw new Error('citation-chunk fixture oracle binding mismatch');
  }
  const caseIds = corpus.cases.map((entry) => entry.id);
  const expectedIds = [
    'citation-named-heading-propagation',
    'semantic-rich-complete-hints',
    'semantic-rich-hints-omitted',
  ];
  if (
    JSON.stringify(caseIds) !== JSON.stringify(expectedIds) ||
    JSON.stringify([...Object.keys(oracle.expectations)].sort()) !==
      JSON.stringify([...expectedIds].sort())
  ) {
    throw new Error('semantic-hint corpus and oracle must contain the exact three case IDs');
  }
  return {
    baselineCommit: commit,
    baselineTree: tree,
    corpusSha256: sha256(readFileSync(corpusPath)),
    oracleSha256: sha256(readFileSync(oraclePath)),
    runnerSha256: sha256(readFileSync(runnerPath)),
    generatorSha256: sha256(readFileSync(generatorPath)),
    fixtureManifestSha256: sha256(readFileSync(fixtureManifestPath)),
    fixtureSha256: fixtureManifest.fixture.sha256,
    citationFixtureSha256: citationFixtureManifest.fixture.sha256,
  };
}

function materialize(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(materialize);
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([key, entry]) => [
        key === 'fixture' ? 'path' : key,
        key === 'fixture' && typeof entry === 'string'
          ? join(fixtureDir, entry)
          : materialize(entry),
      ])
    );
  }
  return value;
}

const authority = verifyAuthority();
if (!existsSync(rustCliPath)) throw new Error('missing release Rust CLI');
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
const observations: Record<string, Json> = {};
for (const entry of corpus.cases) {
  const result = spawnSync(rustCliPath, [], {
    cwd: repoRoot,
    encoding: 'utf8',
    input: JSON.stringify({ tool: 'read_pdf', input: materialize(entry.input) }),
    maxBuffer: 40 * 1024 * 1024,
  });
  if (result.status !== 0) {
    throw new Error(result.stderr || `Rust semantic-hint case exited nonzero: ${entry.id}`);
  }
  if (!result.stdout) throw new Error(result.stderr || 'Rust CLI produced no JSON');
  const envelope = JSON.parse(result.stdout) as Record<string, unknown>;
  const text = (envelope.result as { content?: Array<{ text?: string }> } | undefined)?.content?.[0]
    ?.text;
  if (!text) throw new Error('Rust CLI response lacks structured text');
  const payload = JSON.parse(text) as {
    results: Array<{ success: boolean; data: Record<string, unknown> }>;
  };
  if (!payload.results[0]?.success) throw new Error(`Rust semantic-hint case failed: ${entry.id}`);
  const actual = canonical(payload.results[0].data);
  const expected = oracle.expectations[entry.id]!;
  observations[entry.id] = actual;
  if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
}

const rich = structuredClone(oracle.expectations['semantic-rich-complete-hints']) as {
  elements: Array<{
    semantic_hint: { role: string; confidence: number; signals: string[]; level?: number } | null;
  }>;
};
const withHint = rich.elements.find((element) => element.semantic_hint?.level !== undefined);
if (!withHint?.semantic_hint) throw new Error('oracle lacks complete semantic-hint mutation probe');
const mutationSensitive = {
  role: false,
  confidence: false,
  signals: false,
  level: false,
};
for (const field of Object.keys(mutationSensitive) as Array<keyof typeof mutationSensitive>) {
  const mutated = structuredClone(rich);
  const target = mutated.elements.find((element) => element.semantic_hint?.level !== undefined);
  if (!target?.semantic_hint) throw new Error('semantic mutation target disappeared');
  if (field === 'role') target.semantic_hint.role = `${target.semantic_hint.role}-mutated`;
  if (field === 'confidence') target.semantic_hint.confidence += 0.01;
  if (field === 'signals') target.semantic_hint.signals = [...target.semantic_hint.signals, 'mutated'];
  if (field === 'level') target.semantic_hint.level = (target.semantic_hint.level ?? 0) + 1;
  mutationSensitive[field] = !same(mutated as unknown as Json, rich as unknown as Json);
}
if (Object.values(mutationSensitive).some((value) => !value)) {
  throw new Error('semantic-hint canonical comparison missed a required mutation probe');
}

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(
    `candidate SHA assertion mismatch: expected ${process.env.CANDIDATE_SHA}, executed ${candidateSha}`
  );
}
const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_semantic_hint_result',
  candidateSha,
  ...authority,
  caseCount: corpus.cases.length,
  passed: corpus.cases.length - failures.length,
  skipped: 0,
  mutationSensitive,
  pass: failures.length === 0,
  observations,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
