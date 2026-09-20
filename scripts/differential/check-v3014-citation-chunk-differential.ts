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
const corpusPath = join(scriptDir, 'fixtures/v3014-citation-chunk-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-citation-chunk-oracle.json');
const fixtureManifestPath = join(scriptDir, 'fixtures/v3014-behavior-fixtures.json');
const chunkFixtureManifestPath = join(
  scriptDir,
  'fixtures/v3014-citation-chunk-fixture.json'
);
const runnerPath = join(scriptDir, 'v3014-citation-chunk-baseline-runner.ts');
const generatorPath = join(scriptDir, 'generate-v3014-citation-chunk-fixture.ts');
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
    entrypointSha256: Record<string, string>;
  };
  expectations: Record<string, Json>;
};

const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as { cases: Case[] };
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const fixtureManifest = JSON.parse(readFileSync(fixtureManifestPath, 'utf8')) as {
  fixtures: Array<{ path: string; bytes: number; sha256: string }>;
};
const chunkFixtureManifest = JSON.parse(readFileSync(chunkFixtureManifestPath, 'utf8')) as {
  fixture: { path: string; bytes: number; sha256: string };
};
const sha256 = (value: Uint8Array | string): string =>
  createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => {
  const result = spawnSync('git', args, { cwd: repoRoot });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};
const box = (value: unknown): Json => {
  if (!value || typeof value !== 'object') return null;
  const record = value as Record<string, unknown>;
  const coordinate = (key: string): number => Math.round(Number(record[key]) * 1e9) / 1e9;
  return {
    left: coordinate('left'),
    bottom: coordinate('bottom'),
    right: coordinate('right'),
    top: coordinate('top'),
  };
};
const canonChunk = (chunk: Record<string, unknown>): Json => ({
  id: String(chunk.id),
  page_start: Number(chunk.page_start),
  page_end: Number(chunk.page_end),
  text: String(chunk.text),
  element_ids: (chunk.element_ids ?? []) as Json,
  ...(chunk.strategy === undefined ? {} : { strategy: String(chunk.strategy) }),
  ...(chunk.heading === undefined ? {} : { heading: String(chunk.heading) }),
  ...(chunk.bounding_boxes === undefined
    ? {}
    : { bounding_boxes: (chunk.bounding_boxes as unknown[]).map(box) }),
});
const canonical = (data: Record<string, unknown>): Json => {
  const map = data.document_map as Record<string, unknown> | undefined;
  return {
    has_chunks: Object.hasOwn(data, 'chunks'),
    has_elements: Object.hasOwn(data, 'elements'),
    chunks: ((data.chunks ?? []) as Array<Record<string, unknown>>).map(canonChunk),
    elements: ((data.elements ?? []) as Array<Record<string, unknown>>).map((element) => ({
      id: String(element.id),
      type: String(element.type),
      page: Number(element.page),
      content: String(element.content),
      ...(element.bounding_box === undefined ? {} : { bounding_box: box(element.bounding_box) }),
      ...(element.semantic_hint && typeof element.semantic_hint === 'object'
        ? {
            semantic_role: String(
              (element.semantic_hint as Record<string, unknown>).role ?? ''
            ),
          }
        : {}),
    })),
    document_map:
      map === undefined
        ? null
        : {
            chunks: ((map.chunks ?? []) as Array<Record<string, unknown>>).map(canonChunk),
            pages: ((map.pages ?? []) as Array<Record<string, unknown>>).map((page) => ({
              page: Number(page.page),
              chunk_ids: (page.chunk_ids ?? []) as Json,
            })),
          },
  };
};
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
    throw new Error('citation-chunk generator mismatch');
  }
  for (const [sourceFile, expected] of Object.entries(oracle.baseline.entrypointSha256)) {
    if (sha256(git('show', `${commit}:${sourceFile}`)) !== expected) {
      throw new Error(`baseline entrypoint mismatch: ${sourceFile}`);
    }
  }
  const fixture = fixtureManifest.fixtures.find((entry) =>
    entry.path.endsWith('/v3014-behavior-v1.pdf')
  );
  if (!fixture) throw new Error('behavior fixture is not manifest-bound');
  const fixturePath = join(repoRoot, fixture.path);
  if (!existsSync(fixturePath)) throw new Error('missing citation-chunk fixture');
  const bytes = readFileSync(fixturePath);
  if (bytes.length !== fixture.bytes || sha256(bytes) !== fixture.sha256) {
    throw new Error('citation-chunk fixture digest mismatch');
  }
  const chunkFixturePath = join(repoRoot, chunkFixtureManifest.fixture.path);
  if (!existsSync(chunkFixturePath)) throw new Error('missing boundary fixture');
  const chunkFixtureBytes = readFileSync(chunkFixturePath);
  if (
    chunkFixtureBytes.length !== chunkFixtureManifest.fixture.bytes ||
    sha256(chunkFixtureBytes) !== chunkFixtureManifest.fixture.sha256
  ) {
    throw new Error('citation-chunk boundary fixture digest mismatch');
  }
  const corpusIds = corpus.cases.map((entry) => entry.id).sort();
  const oracleIds = Object.keys(oracle.expectations).sort();
  if (corpusIds.length !== 6 || JSON.stringify(corpusIds) !== JSON.stringify(oracleIds)) {
    throw new Error('citation-chunk corpus and oracle must contain the same six case IDs');
  }
  return {
    baselineCommit: commit,
    baselineTree: tree,
    corpusSha256: sha256(readFileSync(corpusPath)),
    oracleSha256: sha256(readFileSync(oraclePath)),
    runnerSha256: sha256(readFileSync(runnerPath)),
    generatorSha256: sha256(readFileSync(generatorPath)),
    fixtureSha256: fixture.sha256,
    boundaryFixtureSha256: chunkFixtureManifest.fixture.sha256,
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
  if (!result.stdout) throw new Error(result.stderr || 'Rust CLI produced no JSON');
  const envelope = JSON.parse(result.stdout) as Record<string, unknown>;
  const text = (envelope.result as { content?: Array<{ text?: string }> } | undefined)?.content?.[0]
    ?.text;
  if (!text) throw new Error('Rust CLI response lacks structured text');
  const payload = JSON.parse(text) as {
    results: Array<{ success: boolean; data: Record<string, unknown> }>;
  };
  if (!payload.results[0]?.success) throw new Error(`Rust citation-chunk case failed: ${entry.id}`);
  const actual = canonical(payload.results[0].data);
  const expected = oracle.expectations[entry.id]!;
  observations[entry.id] = actual;
  if (JSON.stringify(canonicalJson(actual)) !== JSON.stringify(canonicalJson(expected))) {
    failures.push({ id: entry.id, expected, actual });
  }
}

const firstId = corpus.cases[0]!.id;
const original = oracle.expectations[firstId]!;
const mutated = structuredClone(original) as {
  chunks?: Array<{ id?: string }>;
};
if (!mutated.chunks?.[0]?.id) throw new Error('oracle lacks citation-chunk mutation probe');
mutated.chunks[0].id = `${mutated.chunks[0].id}-mutated`;
if (JSON.stringify(canonicalJson(mutated as Json)) === JSON.stringify(canonicalJson(original))) {
  throw new Error('citation-chunk mutation probe was not rejected by canonical comparison');
}

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(
    `candidate SHA assertion mismatch: expected ${process.env.CANDIDATE_SHA}, executed ${candidateSha}`
  );
}
const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_citation_chunk_result',
  candidateSha,
  ...authority,
  caseCount: corpus.cases.length,
  passed: corpus.cases.length - failures.length,
  skipped: 0,
  chunkMutationSensitive: true,
  pass: failures.length === 0,
  observations,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
