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
const corpusPath = join(scriptDir, 'fixtures/v3014-text-layer-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-text-layer-oracle.json');
const fixtureManifestPath = join(scriptDir, 'fixtures/v3014-behavior-fixtures.json');
const runnerPath = join(scriptDir, 'v3014-text-layer-baseline-runner.ts');
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
    entrypointSha256: Record<string, string>;
  };
  expectations: Record<string, Json>;
};

const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as { cases: Case[] };
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const fixtureManifest = JSON.parse(readFileSync(fixtureManifestPath, 'utf8')) as {
  fixtures: Array<{ path: string; bytes: number; sha256: string }>;
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
const canonChar = (value: Record<string, unknown>): Json => ({
  text: String(value.text),
  char_start: Number(value.char_start),
  char_end: Number(value.char_end),
  run_index: Number(value.run_index),
  is_whitespace: Boolean(value.is_whitespace),
  bounding_box: box(value.bounding_box),
  bounding_box_level: (value.bounding_box_level ?? null) as Json,
});
const canonWord = (value: Record<string, unknown>): Json => ({
  text: String(value.text),
  char_start: Number(value.char_start),
  char_end: Number(value.char_end),
  bounding_box: box(value.bounding_box),
  bounding_box_level: (value.bounding_box_level ?? null) as Json,
});
const canonRun = (value: Record<string, unknown>): Json => ({
  text: String(value.text),
  char_start: Number(value.char_start),
  char_end: Number(value.char_end),
  bounding_box: box(value.bounding_box),
  chars: ((value.chars ?? []) as Array<Record<string, unknown>>).map(canonChar),
});
const canonical = (data: Record<string, unknown>): Json => {
  const layer = data.text_layer as Record<string, unknown>;
  const pages = (layer.pages ?? []) as Array<Record<string, unknown>>;
  const summary = layer.summary as Record<string, unknown>;
  return {
    text_layer: {
      version: String(layer.version),
      profile: String(layer.profile),
      pages: pages.map((page) => ({
        page: Number(page.page),
        text: String(page.text),
        char_count: Number(page.char_count),
        line_count: Number(page.line_count),
        word_count: Number(page.word_count),
        lines: ((page.lines ?? []) as Array<Record<string, unknown>>).map((line) => ({
          text: String(line.text),
          char_start: Number(line.char_start),
          char_end: Number(line.char_end),
          bounding_box: box(line.bounding_box),
          runs: ((line.runs ?? []) as Array<Record<string, unknown>>).map(canonRun),
          words: ((line.words ?? []) as Array<Record<string, unknown>>).map(canonWord),
          chars: ((line.chars ?? []) as Array<Record<string, unknown>>).map(canonChar),
        })),
      })),
      summary: {
        selected_pages: (summary.selected_pages ?? []) as Json,
        page_count: Number(summary.page_count),
        run_count: Number(summary.run_count),
        line_count: Number(summary.line_count),
        word_count: Number(summary.word_count),
        char_count: Number(summary.char_count),
        chars_with_bounding_boxes: Number(summary.chars_with_bounding_boxes),
        runs_with_bounding_boxes: Number(summary.runs_with_bounding_boxes),
        lines_with_bounding_boxes: Number(summary.lines_with_bounding_boxes),
        words_with_bounding_boxes: Number(summary.words_with_bounding_boxes),
      },
    },
    elements: ((data.elements ?? []) as Array<Record<string, unknown>>)
      .filter((element) => element.type === 'text')
      .map((element) => ({
        page: Number(element.page),
        content: String(element.content),
        bounding_box: box(element.bounding_box),
      })),
    chunks: ((data.chunks ?? []) as Array<Record<string, unknown>>).map((chunk) => ({
      bounding_boxes: ((chunk.bounding_boxes ?? []) as unknown[]).map(box),
    })),
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
  if (!existsSync(fixturePath)) throw new Error('missing text-layer fixture');
  const bytes = readFileSync(fixturePath);
  if (bytes.length !== fixture.bytes || sha256(bytes) !== fixture.sha256) {
    throw new Error('text-layer fixture digest mismatch');
  }
  if (corpus.cases.length !== 1 || Object.keys(oracle.expectations).length !== 1) {
    throw new Error('text-layer corpus and oracle must contain exactly one case');
  }
  const corpusIds = corpus.cases.map((entry) => entry.id).sort();
  const oracleIds = Object.keys(oracle.expectations).sort();
  if (JSON.stringify(corpusIds) !== JSON.stringify(oracleIds)) {
    throw new Error('text-layer corpus and oracle case IDs differ');
  }
  return {
    baselineCommit: commit,
    baselineTree: tree,
    corpusSha256: sha256(readFileSync(corpusPath)),
    oracleSha256: sha256(readFileSync(oraclePath)),
    runnerSha256: sha256(readFileSync(runnerPath)),
    fixtureSha256: fixture.sha256,
  };
}

function materialize(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(materialize);
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([key, entry]) => [
        key === 'fixture' ? 'path' : key,
        key === 'fixture' && typeof entry === 'string' ? join(fixtureDir, entry) : materialize(entry),
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
  const payload = JSON.parse(text) as { results: Array<{ success: boolean; data: Record<string, unknown> }> };
  if (!payload.results[0]?.success) throw new Error('Rust read_pdf case failed');
  const actual = canonical(payload.results[0].data);
  const expected = oracle.expectations[entry.id]!;
  observations[entry.id] = actual;
  if (JSON.stringify(canonicalJson(actual)) !== JSON.stringify(canonicalJson(expected))) {
    failures.push({ id: entry.id, expected, actual });
  }
}

const firstId = corpus.cases[0]!.id;
const original = oracle.expectations[firstId]!;
const mutationProbe = structuredClone(original) as {
  text_layer?: { pages?: Array<{ lines?: Array<{ chars?: Array<{ bounding_box?: { right?: number } }> }> }> };
};
const firstBox = mutationProbe.text_layer?.pages?.[0]?.lines?.[0]?.chars?.[0]?.bounding_box;
if (!firstBox || typeof firstBox.right !== 'number') {
  throw new Error('oracle lacks character geometry mutation probe');
}
firstBox.right += 1;
if (JSON.stringify(canonicalJson(mutationProbe as Json)) === JSON.stringify(canonicalJson(original))) {
  throw new Error('geometry mutation probe was not rejected by canonical comparison');
}

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(
    `candidate SHA assertion mismatch: expected ${process.env.CANDIDATE_SHA}, executed ${candidateSha}`
  );
}
const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_text_layer_result',
  candidateSha,
  ...authority,
  caseCount: corpus.cases.length,
  passed: corpus.cases.length - failures.length,
  skipped: 0,
  geometryMutationSensitive: true,
  pass: failures.length === 0,
  observations,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
