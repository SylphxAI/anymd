#!/usr/bin/env bun
/**
 * TRUE TypeScript → pure-Rust text differential (claimed subset only).
 *
 * Oracle = TypeScript 3.0.14 production code paths
 *   (processSingleSource / searchPdfSource)
 * Subject = pure-Rust pdf-reader-cli → pdf-reader-core
 *
 * Claims (and only these):
 *   1. full_text token recall >= 0.70 on sample.pdf
 *   2. search finds query; match_start/match_end snake_case present
 *   3. metadata-only does not auto-enable twin layers on Rust
 *   4. SSRF fail-closed for loopback URL
 *
 * Non-claims: bboxes, render/OCR, exact string equality of full_text.
 *
 * Exit 0 only if all claimed comparisons pass.
 */
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, '../..');
const SAMPLE = join(REPO_ROOT, 'test/fixtures/sample.pdf');
const FIXTURE_OUT = join(__dirname, 'fixtures/ts-text-oracle-baseline.json');
const RUST_CLI = join(resolveCargoReleaseDir(REPO_ROOT), 'pdf-reader-cli');
const writeFixture = process.argv.includes('--write-fixture');

type Failure = { id: string; detail: string };

const normalizeText = (value: string): string =>
  value
    .replace(/\r\n/g, '\n')
    .replace(/[ \t]+/g, ' ')
    .replace(/\n{3,}/g, '\n\n')
    .trim();

async function tsReadFullText(path: string): Promise<string> {
  const { processSingleSource } = await import('../../src/pdf/readCoordinator.ts');
  const { buildReadOptions } = await import('../../src/pdf/autoReadPolicy.ts');
  const { PdfSessionScope } = await import('../../src/pdf/pdfSession.ts');
  const session = new PdfSessionScope();
  const options = buildReadOptions({
    sources: [{ path }],
    auto: false,
    include_full_text: true,
    include_page_count: true,
  });
  const result = await processSingleSource({ path }, options, session);
  if (!result.success || !result.data?.full_text) {
    throw new Error(`TS read failed: ${result.error ?? 'no full_text'}`);
  }
  return result.data.full_text;
}

async function tsSearch(path: string, query: string) {
  const { searchPdfSource, defaultSearchPdfOptions } = await import('../../src/pdf/search.ts');
  const options = {
    ...defaultSearchPdfOptions(query),
    max_pages: 10,
    max_matches_per_source: 20,
    prefer_speed: false,
  };
  const result = await searchPdfSource({ path }, options);
  if (!result.success) {
    throw new Error(`TS search failed: ${result.error}`);
  }
  return result.matches ?? [];
}

function ensureRustCli() {
  if (existsSync(RUST_CLI)) return;
  const build = spawnSync('cargo', ['build', '--release', '-p', 'pdf-reader-cli'], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
  });
  if (build.status !== 0) {
    throw new Error(`build pdf-reader-cli failed:\n${build.stderr}`);
  }
}

function rustCli(tool: string, input: Record<string, unknown>): Record<string, unknown> {
  ensureRustCli();
  const proc = spawnSync(RUST_CLI, [], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    input: JSON.stringify({ tool, input }),
    maxBuffer: 20 * 1024 * 1024,
  });
  if (proc.status !== 0 && !proc.stdout) {
    throw new Error(`rust cli spawn failed: ${proc.stderr}`);
  }
  const parsed = JSON.parse(proc.stdout) as Record<string, unknown>;
  if (parsed.status === 'error') {
    throw new Error(`rust cli error: ${JSON.stringify(parsed)}`);
  }
  return parsed;
}

function unwrapRustPayload(envelope: Record<string, unknown>): Record<string, unknown> {
  // envelope.result.content[0].text is JSON string of core response
  const result = envelope.result as
    | { content?: Array<{ text?: string }>; structuredContent?: Record<string, unknown> }
    | undefined;
  if (result?.structuredContent) return result.structuredContent;
  const text = result?.content?.[0]?.text;
  if (typeof text === 'string') {
    return JSON.parse(text) as Record<string, unknown>;
  }
  // Some envelopes put response at top-level
  if (envelope.results) return envelope;
  throw new Error(`Cannot unwrap rust payload: ${JSON.stringify(envelope).slice(0, 400)}`);
}

function rustReadFullText(path: string): string {
  const envelope = rustCli('read_pdf', {
    sources: [{ path }],
    auto: false,
    include_full_text: true,
    include_page_count: true,
  });
  const payload = unwrapRustPayload(envelope);
  const results = payload.results as Array<{
    success?: boolean;
    data?: { full_text?: string };
    error?: string;
  }>;
  const first = results?.[0];
  if (!first?.success || !first.data?.full_text) {
    throw new Error(`Rust read failed: ${first?.error ?? JSON.stringify(payload).slice(0, 300)}`);
  }
  return first.data.full_text;
}

function rustSearch(path: string, query: string) {
  const envelope = rustCli('search_pdf', {
    sources: [{ path }],
    query,
    max_pages: 10,
    max_matches_per_source: 20,
  });
  const payload = unwrapRustPayload(envelope);
  const results = payload.results as Array<{
    success?: boolean;
    matches?: Array<Record<string, unknown>>;
    error?: string;
  }>;
  const first = results?.[0];
  if (!first?.success) {
    throw new Error(`Rust search failed: ${first?.error ?? 'unknown'}`);
  }
  return first.matches ?? [];
}

function rustMetadataOnlyLeaks(path: string): Record<string, boolean> {
  const envelope = rustCli('read_pdf', {
    sources: [{ path }],
    include_metadata: true,
    include_page_count: true,
  });
  const payload = unwrapRustPayload(envelope);
  const data = (payload.results as Array<{ data?: Record<string, unknown> }>)?.[0]?.data ?? {};
  return {
    full_text: data.full_text != null,
    markdown: data.markdown != null,
    tables: data.tables != null,
    trust_report: data.trust_report != null,
  };
}

function rustSsrfBlocked(): boolean {
  const proc = spawnSync(RUST_CLI, [], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    input: JSON.stringify({
      tool: 'read_pdf',
      input: {
        sources: [{ url: 'http://127.0.0.1/secret.pdf' }],
        auto: false,
        include_full_text: false,
      },
    }),
  });
  try {
    const envelope = JSON.parse(proc.stdout) as { status?: string; message?: string };
    return (
      envelope.status === 'error' &&
      envelope.message?.includes('resolves to a non-public address (SSRF protection)') === true
    );
  } catch {
    return false;
  }
}

async function main() {
  if (!existsSync(SAMPLE)) {
    console.error(`Missing fixture ${SAMPLE}`);
    process.exit(1);
  }

  const failures: Failure[] = [];
  const query = 'Lorem';

  console.error('[ts-vs-rust] TS oracle: processSingleSource / searchPdfSource');
  const tsText = normalizeText(await tsReadFullText(SAMPLE));
  const tsMatches = await tsSearch(SAMPLE, query);

  console.error('[ts-vs-rust] Rust subject: pdf-reader-cli → pdf-reader-core');
  const rustText = normalizeText(rustReadFullText(SAMPLE));
  const rustMatches = rustSearch(SAMPLE, query);

  const tsTokens = new Set(tsText.toLowerCase().split(/\s+/).filter((t) => t.length > 3));
  const rustTokens = new Set(rustText.toLowerCase().split(/\s+/).filter((t) => t.length > 3));
  const shared = [...tsTokens].filter((t) => rustTokens.has(t)).length;
  const recall = tsTokens.size === 0 ? 0 : shared / tsTokens.size;

  if (tsText.length === 0) {
    failures.push({ id: 'ts-full-text', detail: 'TS oracle empty full_text' });
  }
  if (recall < 0.7) {
    failures.push({
      id: 'full-text-token-recall',
      detail: `token recall ${recall.toFixed(3)} < 0.70 (shared=${shared}, ts=${tsTokens.size}, rust=${rustTokens.size})`,
    });
  }

  if (rustMatches.length === 0 && tsMatches.length > 0) {
    failures.push({
      id: 'search-rust-empty',
      detail: `Rust 0 matches; TS ${tsMatches.length} for "${query}"`,
    });
  }
  for (const m of rustMatches) {
    if (m.match_start === undefined || m.match_end === undefined) {
      failures.push({
        id: 'search-snake-case-offsets',
        detail: `missing match_start/match_end: ${JSON.stringify(m)}`,
      });
      break;
    }
  }
  const queryLower = query.toLowerCase();
  if (!rustMatches.some((m) => String(m.text ?? '').toLowerCase().includes(queryLower))) {
    failures.push({
      id: 'search-query-hit',
      detail: `Rust matches do not contain "${query}"`,
    });
  }

  const leaks = rustMetadataOnlyLeaks(SAMPLE);
  if (leaks.full_text || leaks.markdown || leaks.tables || leaks.trust_report) {
    failures.push({
      id: 'metadata-only-auto',
      detail: `leaked twin fields: ${JSON.stringify(leaks)}`,
    });
  }

  if (!rustSsrfBlocked()) {
    failures.push({ id: 'ssrf-loopback', detail: 'did not fail-closed on 127.0.0.1' });
  }

  const report = {
    schemaVersion: 1,
    profile: 'ts-vs-rust-text-claimed-subset',
    oracle: 'TypeScript processSingleSource + searchPdfSource',
    subject: 'pdf-reader-cli → pdf-reader-core',
    fixture: relative(REPO_ROOT, SAMPLE),
    fixtureSha256: createHash('sha256').update(readFileSync(SAMPLE)).digest('hex'),
    measuredAt: new Date().toISOString(),
    claims: [
      'full_text token recall >= 0.70',
      'search hits contain query + snake_case offsets',
      'metadata-only no auto twin',
      'loopback URL rejected with the explicit non-public-address SSRF policy error',
    ],
    nonClaims: ['bounding boxes', 'render/OCR', 'exact full_text equality'],
    stats: {
      tsTextChars: tsText.length,
      rustTextChars: rustText.length,
      tokenRecall: Number(recall.toFixed(4)),
      tsMatchCount: tsMatches.length,
      rustMatchCount: rustMatches.length,
    },
    failures,
    pass: failures.length === 0,
  };

  console.log(JSON.stringify(report, null, 2));
  if (writeFixture) {
    mkdirSync(dirname(FIXTURE_OUT), { recursive: true });
    writeFileSync(FIXTURE_OUT, `${JSON.stringify(report, null, 2)}\n`);
    console.error(`[ts-vs-rust] wrote ${FIXTURE_OUT}`);
  }
  if (failures.length > 0) {
    console.error(`[ts-vs-rust] FAILED ${failures.length}`);
    process.exit(1);
  }
  console.error('[ts-vs-rust] PASS');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
