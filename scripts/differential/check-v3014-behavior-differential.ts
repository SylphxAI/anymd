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
const corpusPath = join(scriptDir, 'fixtures/v3014-behavior-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-behavior-oracle.json');
const fixtureManifestPath = join(scriptDir, 'fixtures/v3014-behavior-fixtures.json');
const baselineRunnerPath = join(scriptDir, 'v3014-baseline-runner.ts');
const rustCliPath = join(resolveCargoReleaseDir(repoRoot), 'pdf-reader-cli');
const outputFlag = process.argv.indexOf('--output');
const outputPath = outputFlag >= 0 ? process.argv[outputFlag + 1] : undefined;

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
type Case = { id: string; tool: string; input: Record<string, unknown> };
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
const sha256 = (bytes: Uint8Array | string): string =>
  createHash('sha256').update(bytes).digest('hex');
const git = (...args: string[]): Buffer => {
  const result = spawnSync('git', args, { cwd: repoRoot });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};
const normalizeText = (value: string): string =>
  value.replaceAll('\r\n', '\n').normalize('NFC');
const normalizeBox = (value: unknown): Json => {
  if (!value || typeof value !== 'object') return null;
  const box = value as Record<string, unknown>;
  const coordinate = (key: string): number => Math.round(Number(box[key]) * 1e9) / 1e9;
  return {
    left: coordinate('left'),
    bottom: coordinate('bottom'),
    right: coordinate('right'),
    top: coordinate('top'),
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
  if (commit !== oracle.baseline.commit) {
    throw new Error(`${oracle.baseline.tag} moved: expected ${oracle.baseline.commit}, got ${commit}`);
  }
  const tree = git('rev-parse', `${commit}^{tree}`).toString().trim();
  if (tree !== oracle.baseline.tree) throw new Error(`baseline tree mismatch: ${tree}`);
  const lockDigest = sha256(git('show', `${commit}:bun.lock`));
  if (lockDigest !== oracle.baseline.bunLockSha256) throw new Error('baseline bun.lock mismatch');
  if (sha256(readFileSync(baselineRunnerPath)) !== oracle.baseline.runnerSha256) {
    throw new Error('baseline runner digest mismatch');
  }
  for (const [path, expected] of Object.entries(oracle.baseline.entrypointSha256)) {
    const actual = sha256(git('show', `${commit}:${path}`));
    if (actual !== expected) throw new Error(`baseline entrypoint mismatch: ${path}`);
  }
  for (const fixture of fixtureManifest.fixtures) {
    const path = join(repoRoot, fixture.path);
    if (!existsSync(path)) throw new Error(`missing fixture: ${fixture.path}`);
    const bytes = readFileSync(path);
    if (bytes.length !== fixture.bytes || sha256(bytes) !== fixture.sha256) {
      throw new Error(`fixture digest mismatch: ${fixture.path}`);
    }
  }
  const ids = corpus.cases.map((entry) => entry.id);
  if (ids.length !== 14 || new Set(ids).size !== ids.length) {
    throw new Error(`behavior corpus must contain 14 unique cases (got ${ids.length})`);
  }
  if (JSON.stringify(ids.sort()) !== JSON.stringify(Object.keys(oracle.expectations).sort())) {
    throw new Error('corpus and oracle case IDs differ');
  }
  return {
    baselineCommit: commit,
    baselineTree: tree,
    corpusSha256: sha256(readFileSync(corpusPath)),
    oracleSha256: sha256(readFileSync(oraclePath)),
    fixtureManifestSha256: sha256(readFileSync(fixtureManifestPath)),
  };
}

function materializeInput(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(materializeInput);
  if (value && typeof value === 'object') {
    const record = value as Record<string, unknown>;
    const mapped: Record<string, unknown> = {};
    for (const [key, entry] of Object.entries(record)) {
      mapped[key === 'fixture' ? 'path' : key] =
        key === 'fixture' && typeof entry === 'string'
          ? join(fixtureDir, entry)
          : materializeInput(entry);
    }
    return mapped;
  }
  return value;
}

function rustCall(entry: Case): Record<string, unknown> {
  if (!existsSync(rustCliPath)) throw new Error(`missing Rust CLI: ${rustCliPath}`);
  const result = spawnSync(rustCliPath, [], {
    cwd: repoRoot,
    encoding: 'utf8',
    input: JSON.stringify({ tool: entry.tool, input: materializeInput(entry.input) }),
    maxBuffer: 20 * 1024 * 1024,
  });
  if (!result.stdout) throw new Error(`Rust CLI produced no JSON: ${result.stderr}`);
  return JSON.parse(result.stdout) as Record<string, unknown>;
}

function unwrapPayload(envelope: Record<string, unknown>): Record<string, unknown> | undefined {
  const result = envelope.result as { content?: Array<{ text?: string }> } | undefined;
  const text = result?.content?.[0]?.text;
  return typeof text === 'string' ? (JSON.parse(text) as Record<string, unknown>) : undefined;
}

function errorCategory(message: string): string {
  const lower = message.toLowerCase();
  if (lower.includes('not found') || lower.includes('no such file') || lower.includes('unable to access')) {
    return 'file_not_found';
  }
  if (lower.includes('pdf') && (lower.includes('parse') || lower.includes('cross reference'))) {
    return 'invalid_pdf';
  }
  return 'unknown_error';
}

function warnings(value: unknown): Json[] {
  if (!Array.isArray(value)) return [];
  return value.map((warning) => {
    const text = String(warning);
    const match = text.match(/(?:page numbers?|pages?)\s+([0-9, ]+).*?(?:total pages|page count)\s*\(?([0-9]+)\)?/i);
    return match
      ? {
          category: 'page_out_of_range',
          requested: match[1]!.split(',').map((part) => Number(part.trim())),
          total: Number(match[2]),
        }
      : { category: 'unknown_warning' };
  });
}

function canonicalRead(id: string, envelope: Record<string, unknown>): Json {
  if (envelope.status === 'error') {
    return { outcome: 'error', category: errorCategory(String(envelope.message ?? '')) };
  }
  const payload = unwrapPayload(envelope);
  const results = payload?.results as Array<Record<string, unknown>> | undefined;
  if (!results) return { outcome: 'error', category: 'invalid_envelope' };
  if (id === 'read-mixed-source-partial') {
    return {
      outcome: 'success',
      results: results.map((result) => {
        const data = result.data as Record<string, unknown> | undefined;
        return result.success === true
          ? { success: true, num_pages: Number(data?.num_pages) }
          : { success: false, category: errorCategory(String(result.error ?? '')) };
      }),
    };
  }
  const first = results[0];
  if (!first?.success) {
    return { outcome: 'error', category: errorCategory(String(first?.error ?? '')) };
  }
  const data = first.data as Record<string, unknown>;
  const canonical: Record<string, Json> = {
    outcome: 'success',
    num_pages: Number(data.num_pages),
  };
  if (id === 'read-all-metadata') {
    const info = data.info as Record<string, unknown> | undefined;
    canonical.info = Object.fromEntries(
      ['PDFFormatVersion', 'Title', 'Author', 'Subject', 'Keywords', 'Creator', 'Producer'].map(
        (key) => [key, (info?.[key] ?? null) as Json]
      )
    );
    canonical.full_text = normalizeText(String(data.full_text ?? ''));
  }
  if (id.startsWith('read-pages-')) {
    canonical.page_texts = Array.isArray(data.page_texts)
      ? data.page_texts.map((page) => ({
          page: Number((page as Record<string, unknown>).page),
          text: normalizeText(String((page as Record<string, unknown>).text ?? '')),
        }))
      : [];
  }
  if (id === 'read-page-signals') {
    canonical.page_geometry = Array.isArray(data.page_geometry)
      ? data.page_geometry.map((entry) => {
          const geometry = entry as Record<string, unknown>;
          const box = geometry.view_box as Record<string, unknown>;
          return {
            page: Number(geometry.page), width: Number(geometry.width), height: Number(geometry.height),
            rotation: Number(geometry.rotation), user_unit: Number(geometry.user_unit),
            view_box: { left: Number(box.left), bottom: Number(box.bottom), right: Number(box.right), top: Number(box.top) },
          };
        })
      : null;
    canonical.annotations = Array.isArray(data.annotations)
      ? data.annotations.map((group) => {
          const value = group as Record<string, unknown>;
          return {
            page: Number(value.page),
            annotations: ((value.annotations ?? []) as Array<Record<string, unknown>>).map((entry) => {
              const box = entry.bounding_box as Record<string, unknown>;
              return {
                page: Number(entry.page), id: String(entry.id), subtype: String(entry.subtype),
                contents: String(entry.contents), url: String(entry.url),
                bounding_box: { left: Number(box.left), bottom: Number(box.bottom), right: Number(box.right), top: Number(box.top) },
              };
            }),
          };
        })
      : null;
  }
  if (id === 'read-catalog-signals') {
    canonical.outline = (data.outline ?? null) as Json;
    canonical.page_labels = (data.page_labels ?? null) as Json;
    canonical.permissions = (data.permissions ?? null) as Json;
    canonical.mark_info = (data.mark_info ?? null) as Json;
  }
  if (id === 'read-forms') {
    canonical.form_fields = (data.form_fields ?? null) as Json;
    canonical.attachments = (data.attachments ?? null) as Json;
  }
  if (id === 'read-attachments') {
    canonical.attachments = (data.attachments ?? null) as Json;
    canonical.form_fields = (data.form_fields ?? null) as Json;
  }
  canonical.warnings = warnings(data.warnings);
  return canonical;
}

function canonicalSearch(envelope: Record<string, unknown>): Json {
  const payload = unwrapPayload(envelope);
  const first = (payload?.results as Array<Record<string, unknown>> | undefined)?.[0];
  if (!first?.success) {
    return { outcome: 'error', category: errorCategory(String(first?.error ?? envelope.message ?? '')) };
  }
  return {
    outcome: 'success',
    num_pages: Number(first.num_pages),
    searched_pages: (first.searched_pages ?? []) as Json,
    matches: ((first.matches ?? []) as Array<Record<string, unknown>>).map((match) => ({
      page: Number(match.page),
      text: normalizeText(String(match.text ?? '')),
      match_start: Number(match.match_start),
      match_end: Number(match.match_end),
      text_item_index: Number(match.text_item_index),
      bounding_box: normalizeBox(match.bounding_box),
      bounding_box_level: (match.bounding_box_level ?? null) as Json,
    })),
  };
}

const authority = verifyAuthority();
const expectedGeometry = Object.values(oracle.expectations)
  .flatMap((entry) =>
    entry && typeof entry === 'object' && !Array.isArray(entry)
      ? (((entry as Record<string, Json>).matches ?? []) as Json[])
      : []
  )
  .find((match) => match && typeof match === 'object' && !Array.isArray(match) && (match as Record<string, Json>).bounding_box !== null);
if (!expectedGeometry || typeof expectedGeometry !== 'object' || Array.isArray(expectedGeometry)) {
  throw new Error('behavior oracle must bind at least one search bounding box');
}
const mutatedGeometry = structuredClone(expectedGeometry) as Record<string, Json>;
const mutatedBox = mutatedGeometry.bounding_box as Record<string, Json>;
mutatedBox.right = Number(mutatedBox.right) + 1;
if (JSON.stringify(canonicalJson(mutatedGeometry)) === JSON.stringify(canonicalJson(expectedGeometry))) {
  throw new Error('behavior normalizer failed bounding-box mutation sensitivity');
}
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
const observations: Record<string, Json> = {};
for (const entry of corpus.cases) {
  const envelope = rustCall(entry);
  const actual = entry.tool === 'read_pdf' ? canonicalRead(entry.id, envelope) : canonicalSearch(envelope);
  const expected = oracle.expectations[entry.id]!;
  observations[entry.id] = actual;
  // JSON object member ordering is not semantic; array ordering and exact
  // field presence/value shapes remain strict.
  if (JSON.stringify(canonicalJson(actual)) !== JSON.stringify(canonicalJson(expected))) {
    failures.push({ id: entry.id, expected, actual });
  }
}
const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(
    `candidate SHA assertion mismatch: expected ${process.env.CANDIDATE_SHA}, executed ${candidateSha}`
  );
}
const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_behavior_result',
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
