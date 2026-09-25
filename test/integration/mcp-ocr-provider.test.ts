import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import type { ChildProcess } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import {
  callTool,
  ensureProductionArtifacts,
  initializeSession,
  parseToolPayload,
  spawnProductionMcp,
} from '../production/mcpContract.helpers.js';

const repoRoot = path.resolve(import.meta.dirname, '../..');
const fixture = path.join(repoRoot, 'test/fixtures/differential/v3014-visual-v1.pdf');
const provider = path.join(repoRoot, 'test/fixtures/providers/reference-ocr-provider.ts');

describe('pure-Rust command OCR provider integration', () => {
  let proc: ChildProcess;
  let failingProc: ChildProcess;
  let timeoutProc: ChildProcess;
  let descendantProc: ChildProcess;
  let ordinalProc: ChildProcess;
  const descendantWorkspace = mkdtempSync(path.join(tmpdir(), 'pdf-reader-ocr-descendant-'));
  const descendantMarker = path.join(descendantWorkspace, 'input-path.txt');
  const ordinalWorkspace = mkdtempSync(path.join(tmpdir(), 'pdf-reader-ocr-ordinal-'));
  const ordinalProvider = path.join(ordinalWorkspace, 'fail-first-provider.mjs');
  const ordinalMarker = path.join(ordinalWorkspace, 'invocations.txt');

  beforeAll(async () => {
    ensureProductionArtifacts();
    proc = spawnProductionMcp({
      PDF_READER_ENGINE_MODE: 'pure-rust',
      MCP_PDF_OCR_COMMAND: process.execPath,
      MCP_PDF_OCR_ARGS_JSON: JSON.stringify([provider, '{input}', '{page}', '{languages}']),
    });
    await initializeSession(proc, 'rust-ocr-provider-contract');

    failingProc = spawnProductionMcp({
      PDF_READER_ENGINE_MODE: 'pure-rust',
      MCP_PDF_OCR_COMMAND: process.execPath,
      MCP_PDF_OCR_ARGS_JSON: JSON.stringify([provider, '{input}', '{page}', '{languages}', 'fail']),
    });
    await initializeSession(failingProc, 'rust-ocr-provider-failure-contract');

    timeoutProc = spawnProductionMcp({
      PDF_READER_ENGINE_MODE: 'pure-rust',
      MCP_PDF_OCR_COMMAND: process.execPath,
      MCP_PDF_OCR_ARGS_JSON: JSON.stringify([
        provider,
        '{input}',
        '{page}',
        '{languages}',
        'sleep',
      ]),
    });
    await initializeSession(timeoutProc, 'rust-ocr-provider-timeout-contract');

    descendantProc = spawnProductionMcp({
      PDF_READER_ENGINE_MODE: 'pure-rust',
      MCP_PDF_OCR_COMMAND: process.execPath,
      MCP_PDF_OCR_ARGS_JSON: JSON.stringify([
        provider,
        '{input}',
        '{page}',
        '{languages}',
        'escaped-descendant',
        descendantMarker,
      ]),
    });
    await initializeSession(descendantProc, 'rust-ocr-provider-descendant-contract');

    writeFileSync(
      ordinalProvider,
      `import fs from 'node:fs';
const [, page = '0', marker] = process.argv.slice(2);
const previous = fs.existsSync(marker) ? Number(fs.readFileSync(marker, 'utf8')) : 0;
fs.writeFileSync(marker, String(previous + 1));
if (previous === 0) process.exit(7);
process.stdout.write(JSON.stringify({ text: 'Ordinal OCR page ' + page, confidence: 0.9, words: [] }));
`
    );
    ordinalProc = spawnProductionMcp({
      PDF_READER_ENGINE_MODE: 'pure-rust',
      MCP_PDF_OCR_COMMAND: process.execPath,
      MCP_PDF_OCR_ARGS_JSON: JSON.stringify([ordinalProvider, '{input}', '{page}', ordinalMarker]),
    });
    await initializeSession(ordinalProc, 'rust-ocr-provider-ordinal-contract');
  }, 420_000);

  afterAll(() => {
    proc?.kill('SIGTERM');
    failingProc?.kill('SIGTERM');
    timeoutProc?.kill('SIGTERM');
    descendantProc?.kill('SIGTERM');
    ordinalProc?.kill('SIGTERM');
    rmSync(descendantWorkspace, { recursive: true, force: true });
    rmSync(ordinalWorkspace, { recursive: true, force: true });
  });

  test('renders a bounded page and returns normalized OCR provenance', async () => {
    const response = await callTool(
      proc,
      41,
      'pdf_evidence',
      {
        operation: 'ocr_pages',
        sources: [{ path: fixture, pages: [1] }],
        scale: 2,
        max_pages: 1,
        languages: ['fra', 'eng'],
      },
      90_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(false);
    expect(response.result?.content).toHaveLength(1);
    const result = JSON.parse(payload.text) as {
      profile: string;
      ocr_options: Record<string, unknown>;
      results: Array<{
        success: boolean;
        ocr_pages: Array<Record<string, unknown>>;
      }>;
    };
    expect(result.profile).toBe('ocr_text_layer');
    expect(result.ocr_options).toMatchObject({
      scale: 2,
      max_pages: 1,
      languages: ['fra', 'eng'],
    });
    expect(result.results[0]?.success).toBe(true);
    expect(result.results[0]?.ocr_pages[0]).toMatchObject({
      page: 1,
      text: 'Reference OCR page 1 at 240x160',
      confidence: 0.87,
      language: 'fra',
      provider: 'command',
      source_render_evidence_id: 'page-1-render-scale-2',
      source_render_scale: 2,
      source_render_width: 240,
      source_render_height: 160,
      provenance: { engine: 'external-command', source: 'ocr-provider' },
      words: [
        {
          text: 'Reference',
          confidence: 0.91,
          bounding_box: { left: 10, bottom: 5, right: 50, top: 15 },
        },
      ],
    });
  }, 120_000);

  test('fuses command OCR into read_pdf without mutating selectable text surfaces', async () => {
    const response = await callTool(
      proc,
      45,
      'read_pdf',
      {
        sources: [{ path: fixture, pages: [1] }],
        auto: false,
        include_full_text: true,
        include_document_map: true,
        include_ocr_text_layer: true,
      },
      90_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(false);
    const structured = response.result?.structuredContent as
      | {
          evidence?: { confidence?: string };
          results?: Array<{
            success?: boolean;
            data?: {
              full_text?: string;
              page_texts?: Array<{ page?: number; text?: string }>;
              ocr_text_layer?: {
                profile?: string;
                pages?: Array<Record<string, unknown>>;
                summary?: Record<string, unknown>;
              };
              document_map?: {
                layers?: string[];
                routing?: { needs_ocr_pages?: number[]; ocr_applied_pages?: number[] };
              };
            };
          }>;
        }
      | undefined;
    const data = structured?.results?.[0]?.data;
    expect(structured?.results?.[0]?.success).toBe(true);
    expect(data?.full_text).toBeUndefined();
    expect(data?.page_texts?.[0]?.page).toBe(1);
    expect(JSON.stringify(data?.page_texts)).not.toContain('Reference OCR page');
    expect(data?.ocr_text_layer).toMatchObject({
      profile: 'ocr_text_layer',
      pages: [
        {
          page: 1,
          text: 'Reference OCR page 1 at 240x160',
          confidence: 0.87,
          provider: 'command',
          source_render_evidence_id: 'page-1-render-scale-2',
          provenance: { engine: 'external-command', source: 'ocr-provider' },
        },
      ],
      summary: {
        page_count: 1,
        text_chars: 31,
        word_count: 1,
        words_with_bounding_boxes: 1,
        source_render_count: 1,
        average_confidence: 0.87,
      },
    });
    expect(data?.document_map?.layers).toContain('ocr_text_layer');
    expect(data?.document_map?.routing).toMatchObject({
      needs_ocr_pages: [1],
      ocr_applied_pages: [1],
    });
    expect(structured?.evidence?.confidence).toBe('provider-dependent');
    expect(response.result?.content?.at(-1)?.text).toBe(
      '[Page 1 OCR]\nReference OCR page 1 at 240x160'
    );
  }, 120_000);

  test('keeps read_pdf source successful and omits the layer when OCR fails', async () => {
    const response = await callTool(
      failingProc,
      46,
      'read_pdf',
      {
        sources: [{ path: fixture, pages: [1] }],
        auto: false,
        include_full_text: true,
        include_ocr_text_layer: true,
      },
      30_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(false);
    const structured = response.result?.structuredContent as {
      results?: Array<{
        success?: boolean;
        data?: { ocr_text_layer?: unknown; warnings?: string[] };
      }>;
    };
    const result = structured.results?.[0];
    expect(result?.success).toBe(true);
    expect(result?.data?.ocr_text_layer).toBeUndefined();
    expect(result?.data?.warnings).toContain(
      'OCR text layer unavailable: OCR provider command failed for page 1.'
    );
    expect(response.result?.content?.some((part) => part.text?.startsWith('[Page 1 OCR]'))).toBe(
      false
    );
  }, 30_000);

  test('keeps duplicate source labels aligned when early OCR fails and later OCR succeeds', async () => {
    const response = await callTool(
      ordinalProc,
      47,
      'read_pdf',
      {
        sources: [
          { path: fixture, pages: [1] },
          { path: fixture, pages: [1] },
        ],
        auto: false,
        include_document_map: true,
        include_ocr_text_layer: true,
      },
      90_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(false);
    const structured = response.result?.structuredContent as {
      results?: Array<{
        source?: string;
        success?: boolean;
        data?: {
          ocr_text_layer?: { pages?: Array<{ text?: string }> };
          document_map?: { routing?: Record<string, unknown> };
          warnings?: string[];
        };
      }>;
    };
    const results = structured.results ?? [];
    expect(results).toHaveLength(2);
    expect(results[0]?.source).toBe(results[1]?.source);
    expect(results[0]?.success).toBe(true);
    expect(results[0]?.data?.ocr_text_layer).toBeUndefined();
    expect(results[0]?.data?.warnings).toContain(
      'OCR text layer unavailable: OCR provider command failed for page 1.'
    );
    expect(results[0]?.data?.document_map?.routing).toMatchObject({
      needs_ocr_pages: [1],
      ocr_applied_pages: [],
    });
    expect(results[1]?.success).toBe(true);
    expect(results[1]?.data?.ocr_text_layer?.pages?.[0]?.text).toBe('Ordinal OCR page 1');
    expect(results[1]?.data?.document_map?.routing).toMatchObject({
      needs_ocr_pages: [1],
      ocr_applied_pages: [1],
    });
    expect(readFileSync(ordinalMarker, 'utf8')).toBe('2');
  }, 120_000);

  test('fails closed when the provider exits unsuccessfully', async () => {
    const response = await callTool(
      failingProc,
      42,
      'pdf_evidence',
      {
        operation: 'ocr_pages',
        sources: [{ path: fixture, pages: [1] }],
        scale: 1,
        max_pages: 1,
      },
      30_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(true);
    expect(payload.text).toContain('All PDF sources failed OCR');
    expect(payload.text).toContain('OCR provider command failed for page 1');
  });

  test('kills and fails closed when the provider exceeds its timeout', async () => {
    const started = Date.now();
    const response = await callTool(
      timeoutProc,
      43,
      'pdf_evidence',
      {
        operation: 'ocr_pages',
        sources: [{ path: fixture, pages: [1] }],
        scale: 1,
        max_pages: 1,
        timeout_ms: 1_000,
      },
      30_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(true);
    expect(payload.text).toContain('OCR provider command timed out for page 1');
    expect(Date.now() - started).toBeLessThan(4_000);
  }, 30_000);

  test('bounds escaped inherited-pipe descendants and cleans the temporary page', async () => {
    const started = Date.now();
    const response = await callTool(
      descendantProc,
      44,
      'pdf_evidence',
      {
        operation: 'ocr_pages',
        sources: [{ path: fixture, pages: [1] }],
        scale: 1,
        max_pages: 1,
        timeout_ms: 1_000,
      },
      30_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(true);
    expect(payload.text).toContain('OCR provider command timed out for page 1');
    expect(Date.now() - started).toBeLessThan(4_000);
    const temporaryInput = readFileSync(descendantMarker, 'utf8');
    expect(existsSync(temporaryInput)).toBe(false);
  }, 30_000);
});
