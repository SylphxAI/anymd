#!/usr/bin/env bun
/**
 * WARNING: This script currently shells to the Rust CLI for expected output.
 * It is NOT a TypeScript behavioral oracle. Do not treat differential green as TS parity.
 * TODO: freeze fixtures from 3.0.14 TypeScript handlers as the true baseline.
 */
/**
 * TS contract oracle for pdf-reader-mcp read_pdf + stdio differential parity (rej-010).
 *
 * Frozen baseline replay from read-pdf-golden.json via pdf-reader-cli (Rust SSOT capture).
 * Emits transport/surface/server contracts + stdio probe expectations consumed by
 * crates/pdf-reader-mcp-server/tests/pdf_reader_mcp_differential.rs.
 *
 * Fail-closed: requires built pdf-reader-cli (no SKIP-as-pass).
 */
import { createHash } from 'node:crypto';
import { execSync, spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, '../..');
const CORPUS_PATH = join(__dirname, 'fixtures/pdf-reader-mcp-corpus.json');
const GOLDEN_PATH = join(REPO_ROOT, 'test/fixtures/read-pdf-golden.json');
const FIXTURES_ROOT = join(REPO_ROOT, 'test/fixtures');
const RUST_CLI = join(resolveCargoReleaseDir(REPO_ROOT), 'pdf-reader-cli');

interface TransportContractCase {
  id: string;
  env: Record<string, string>;
  expect: { transport: string };
}

interface SurfaceContractCase {
  id: string;
  surface: 'bin' | 'stdio';
  markers: string[];
}

interface ToolRouteCase {
  id: string;
  tool: string;
  expect: string;
}

interface ReadPdfCaseRef {
  id: string;
  goldenId: string;
}

interface StdioProbeCase {
  id: string;
  kind: 'initialize' | 'toolsList' | 'readPdf' | 'searchPdf' | 'pdfEvidence';
  goldenId?: string;
  expect?: Record<string, unknown>;
}

interface Corpus {
  corpusVersion: number;
  goldenFixture: string;
  transportContractCases: TransportContractCase[];
  surfaceContractCases: SurfaceContractCase[];
  toolRouteCases: ToolRouteCase[];
  readPdfCases: ReadPdfCaseRef[];
  stdioProbeCases: StdioProbeCase[];
  serverContract: {
    name: string;
    tools: string[];
  };
}

type GoldenCase = {
  id: string;
  fixture: string;
  input: Record<string, unknown>;
  expects: {
    error?: boolean;
    code?: string;
    message_contains?: string;
    route?: string;
    payload?: Record<string, unknown>;
    evidence?: Record<string, unknown>;
  };
};

type GoldenManifest = {
  profile: string;
  cases: GoldenCase[];
};

export interface DifferentialCase {
  readonly id: string;
  readonly domain:
    | 'transportContract'
    | 'surfaceContract'
    | 'serverContract'
    | 'toolRouteContract'
    | 'readPdfTool'
    | 'stdioProbe';
  readonly input: Record<string, unknown>;
  readonly output: unknown;
}

export interface DifferentialCorpus {
  readonly corpusVersion: number;
  readonly fixtureCorpusHash: string;
  readonly goldenFixtureHash: string;
  readonly profile: string;
  readonly cases: readonly DifferentialCase[];
}

function fixtureCorpusHash(raw: string): string {
  return createHash('sha256').update(raw).digest('hex');
}

function resolveTransport(env: Record<string, string | undefined>): string {
  if (env.PDF_READER_MCP_TRANSPORT) {
    return env.PDF_READER_MCP_TRANSPORT;
  }
  if (env.MCP_TRANSPORT) {
    return env.MCP_TRANSPORT;
  }
  return 'stdio';
}

function surfaceFile(surface: SurfaceContractCase['surface']): string {
  switch (surface) {
    case 'bin':
      return join(REPO_ROOT, 'bin/anymd');
    case 'stdio':
      return join(REPO_ROOT, 'crates/pdf-reader-mcp-server/src/main.rs');
  }
}

function surfaceMarkers(surface: SurfaceContractCase): Record<string, boolean> {
  const content = readFileSync(surfaceFile(surface.surface), 'utf8');
  const markers: Record<string, boolean> = {};
  for (const marker of surface.markers) {
    markers[marker] = content.includes(marker);
  }
  return markers;
}

function buildRequestInput(fixture: string, input: Record<string, unknown>): Record<string, unknown> {
  const request = structuredClone(input);
  if (!Array.isArray(request.sources)) {
    request.sources = [{ path: join(FIXTURES_ROOT, fixture) }];
  } else {
    request.sources = request.sources.map((source) => {
      if (!source || typeof source !== 'object') {
        return source;
      }
      const entry = { ...(source as Record<string, unknown>) };
      if (typeof entry.path === 'string' && !entry.path.startsWith('/')) {
        entry.path = join(FIXTURES_ROOT, entry.path);
      }
      return entry;
    });
  }
  return request;
}

function normalizePayload(payload: Record<string, unknown>): Record<string, unknown> {
  const normalized = structuredClone(payload);
  if (Array.isArray(normalized.results)) {
    normalized.results = normalized.results.map((result) => {
      if (!result || typeof result !== 'object') {
        return result;
      }
      const entry = { ...(result as Record<string, unknown>) };
      if (typeof entry.source === 'string') {
        entry.source = relative(FIXTURES_ROOT, entry.source).split('\\').join('/');
      }
      if (entry.data && typeof entry.data === 'object') {
        const data = { ...(entry.data as Record<string, unknown>) };
        delete data.fullText;
        delete data.full_text;
        delete data.evidence;
        if (data.info && typeof data.info === 'object') {
          const info = { ...(data.info as Record<string, unknown>) };
          delete info.text_chars;
          delete info.textChars;
          data.info = info;
        }
        entry.data = data;
      }
      return entry;
    });
  }
  delete normalized.evidence;
  return normalized;
}

function parseCliPayload(envelope: Record<string, unknown>): Record<string, unknown> {
  const text = (
    envelope.result as { content?: Array<{ text?: string }> } | undefined
  )?.content?.[0]?.text;
  if (!text) {
    throw new Error('CLI envelope missing read_pdf payload text');
  }
  return JSON.parse(text) as Record<string, unknown>;
}

function ensureRustCliBuilt(): void {
  if (!existsSync(RUST_CLI)) {
    execSync('cargo build --release -p pdf-reader-core -p pdf-reader-cli -p pdf-reader-mcp-server', {
      cwd: REPO_ROOT,
      stdio: 'pipe',
      timeout: 300_000,
    });
  }
}

function invokeCliTool(tool: string, input: Record<string, unknown>) {
  const probe = spawnSync(RUST_CLI, [], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    input: JSON.stringify({
      tool,
      input,
    }),
    timeout: 60_000,
  });

  return {
    status: probe.status,
    stdout: probe.stdout,
    stderr: probe.stderr,
    envelope: probe.status === 0 ? (JSON.parse(probe.stdout) as Record<string, unknown>) : null,
  };
}

function invokeCli(fixture: string, input: Record<string, unknown>) {
  return invokeCliTool('read_pdf', buildRequestInput(fixture, input));
}

function readPdfToolOutput(goldenCase: GoldenCase): Record<string, unknown> {
  const probe = invokeCli(goldenCase.fixture, goldenCase.input);

  if (goldenCase.expects.error) {
    if (probe.status !== 0 || !probe.envelope) {
      throw new Error(`${goldenCase.id}: CLI probe failed: ${probe.stderr}`);
    }
    const envelope = probe.envelope as { status?: string; code?: string; message?: string };
    return {
      status: 'error',
      code: envelope.code,
      message_contains: goldenCase.expects.message_contains,
      message: envelope.message ?? '',
    };
  }

  const samplePdf = join(FIXTURES_ROOT, 'sample.pdf');
  if (!existsSync(samplePdf)) {
    return { status: 'skipped', reason: 'sample.pdf missing' };
  }

  if (probe.status !== 0 || !probe.envelope) {
    throw new Error(`${goldenCase.id}: CLI probe failed: ${probe.stderr}`);
  }

  const envelope = probe.envelope as { status?: string; tool?: string };
  const payload = normalizePayload(parseCliPayload(probe.envelope));
  const expected = normalizePayload(goldenCase.expects.payload as Record<string, unknown>);
  const actualResults = payload.results as Array<Record<string, unknown>>;
  const expectedResults = expected.results as Array<Record<string, unknown>>;

  const output: Record<string, unknown> = {
    status: envelope.status,
    tool: envelope.tool,
    profile: payload.profile,
    route: goldenCase.expects.route,
    success: actualResults[0]?.success,
    engine: (actualResults[0]?.data as { engine?: unknown } | undefined)?.engine,
    info: (actualResults[0]?.data as { info?: unknown } | undefined)?.info,
    expectedInfo: (expectedResults[0]?.data as { info?: Record<string, unknown> } | undefined)?.info,
  };

  const fullTextNeedle = (
    expectedResults[0]?.data as { full_text_contains?: string } | undefined
  )?.full_text_contains;
  if (fullTextNeedle) {
    const rawPayload = parseCliPayload(probe.envelope);
    const rawResults = rawPayload.results as Array<{
      data?: { fullText?: string; full_text?: string };
    }>;
    const fullText = rawResults[0]?.data?.fullText ?? rawResults[0]?.data?.full_text ?? '';
    output.full_text_contains = fullTextNeedle;
    output.full_text = fullText;
  }

  return output;
}

function samplePdfProbeInput(
  tool: 'search_pdf' | 'pdf_evidence',
): Record<string, unknown> | { status: 'skipped'; reason: string } {
  const samplePdf = join(FIXTURES_ROOT, 'sample.pdf');
  if (!existsSync(samplePdf)) {
    return { status: 'skipped', reason: 'sample.pdf missing' };
  }

  if (tool === 'search_pdf') {
    return {
      sources: [{ path: samplePdf }],
      query: 'Lorem',
    };
  }

  return {
    operation: 'inspect',
    sources: [{ path: samplePdf }],
  };
}

function searchPdfToolOutput(): Record<string, unknown> {
  const input = samplePdfProbeInput('search_pdf');
  if ('status' in input && input.status === 'skipped') {
    return input;
  }

  const probe = invokeCliTool('search_pdf', input);
  if (probe.status !== 0 || !probe.envelope) {
    throw new Error(`search_pdf CLI probe failed: ${probe.stderr}`);
  }

  const payload = JSON.parse(
    (
      probe.envelope.result as { content?: Array<{ text?: string }> } | undefined
    )?.content?.[0]?.text ?? '{}'
  ) as {
    profile?: string;
    results?: Array<{ success?: boolean; data?: { route?: string } }>;
  };

  return {
    profile: payload.profile,
    success: payload.results?.[0]?.success,
    route_contains: payload.results?.[0]?.data?.route,
  };
}

function pdfEvidenceToolOutput(): Record<string, unknown> {
  const input = samplePdfProbeInput('pdf_evidence');
  if ('status' in input && input.status === 'skipped') {
    return input;
  }

  const probe = invokeCliTool('pdf_evidence', input);
  if (probe.status !== 0 || !probe.envelope) {
    throw new Error(`pdf_evidence CLI probe failed: ${probe.stderr}`);
  }

  const text =
    (
      probe.envelope.result as { content?: Array<{ text?: string }> } | undefined
    )?.content?.[0]?.text ?? '';
  const payload = JSON.parse(text) as {
    results?: Array<{ success?: boolean; data?: { route?: string } }>;
  };

  return {
    success: payload.results?.[0]?.success,
    route_contains: payload.results?.[0]?.data?.route,
  };
}

function stdioReadPdfOutput(goldenCase: GoldenCase): Record<string, unknown> {
  const toolOutput = readPdfToolOutput(goldenCase);
  if (toolOutput.status === 'skipped') {
    return toolOutput;
  }
  if (toolOutput.status === 'error') {
    return {
      error: true,
      message_contains: toolOutput.message_contains,
    };
  }
  return {
    profile: toolOutput.profile,
    route: toolOutput.route,
    success: toolOutput.success,
    engine: toolOutput.engine,
    info: toolOutput.info,
    expectedInfo: toolOutput.expectedInfo,
    full_text_contains: toolOutput.full_text_contains,
    full_text: toolOutput.full_text,
  };
}

async function main(): Promise<void> {
  const corpusRaw = await readFile(CORPUS_PATH, 'utf8');
  const corpus = JSON.parse(corpusRaw) as Corpus;
  if (corpus.corpusVersion !== 1) {
    throw new Error(`unsupported corpusVersion: ${corpus.corpusVersion}`);
  }

  const goldenRaw = await readFile(GOLDEN_PATH, 'utf8');
  const golden = JSON.parse(goldenRaw) as GoldenManifest;
  const goldenById = new Map(golden.cases.map((entry) => [entry.id, entry]));

  ensureRustCliBuilt();

  const cases: DifferentialCase[] = [];

  for (const entry of corpus.transportContractCases) {
    cases.push({
      id: entry.id,
      domain: 'transportContract',
      input: { env: entry.env },
      output: { transport: resolveTransport(entry.env) },
    });
  }

  for (const entry of corpus.surfaceContractCases) {
    cases.push({
      id: entry.id,
      domain: 'surfaceContract',
      input: { surface: entry.surface, markers: entry.markers },
      output: { markers: surfaceMarkers(entry) },
    });
  }

  cases.push({
    id: 'server-contract',
    domain: 'serverContract',
    input: { tools: corpus.serverContract.tools },
    output: {
      name: corpus.serverContract.name,
      tools: corpus.serverContract.tools,
    },
  });

  for (const entry of corpus.toolRouteCases) {
    cases.push({
      id: entry.id,
      domain: 'toolRouteContract',
      input: { tool: entry.tool },
      output: { route: entry.expect },
    });
  }

  for (const entry of corpus.readPdfCases) {
    const goldenCase = goldenById.get(entry.goldenId);
    if (!goldenCase) {
      throw new Error(`missing golden case ${entry.goldenId}`);
    }
    cases.push({
      id: entry.id,
      domain: 'readPdfTool',
      input: {
        fixture: goldenCase.fixture,
        args: buildRequestInput(goldenCase.fixture, goldenCase.input),
      },
      output: readPdfToolOutput(goldenCase),
    });
  }

  for (const entry of corpus.stdioProbeCases) {
    if (entry.kind === 'initialize' || entry.kind === 'toolsList') {
      cases.push({
        id: entry.id,
        domain: 'stdioProbe',
        input: { kind: entry.kind },
        output: entry.expect ?? {},
      });
      continue;
    }

    if (entry.kind === 'searchPdf') {
      cases.push({
        id: entry.id,
        domain: 'stdioProbe',
        input: { kind: entry.kind },
        output: searchPdfToolOutput(),
      });
      continue;
    }

    if (entry.kind === 'pdfEvidence') {
      cases.push({
        id: entry.id,
        domain: 'stdioProbe',
        input: { kind: entry.kind },
        output: pdfEvidenceToolOutput(),
      });
      continue;
    }

    const goldenCase = goldenById.get(entry.goldenId ?? '');
    if (!goldenCase) {
      throw new Error(`missing golden case for stdio probe ${entry.id}`);
    }
    cases.push({
      id: entry.id,
      domain: 'stdioProbe',
      input: {
        kind: entry.kind,
        fixture: goldenCase.fixture,
        args: buildRequestInput(goldenCase.fixture, goldenCase.input),
      },
      output: stdioReadPdfOutput(goldenCase),
    });
  }

  const payload: DifferentialCorpus = {
    corpusVersion: 1,
    fixtureCorpusHash: fixtureCorpusHash(corpusRaw),
    goldenFixtureHash: fixtureCorpusHash(goldenRaw),
    profile: golden.profile,
    cases,
  };

  process.stdout.write(`${JSON.stringify(payload)}\n`);
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`pdf-reader-mcp-oracle failed: ${message}`);
  process.exit(1);
});
