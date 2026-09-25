/**
 * Sole-Rust production capability contract (ADR-0005/0006).
 *
 * Exercises the production sole-Rust path (runtime-entry + native binary).
 * Key-presence alone is insufficient for full showhand admission; assertions mix
 * presence + selected semantic checks. Broader agent-task/quality closeout remains required.
 */

import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import type { ChildProcess } from 'node:child_process';
import { join } from 'node:path';
import {
  callTool,
  ensureProductionArtifacts,
  initializeSession,
  parseToolPayload,
  samplePdf,
  spawnProductionMcp,
} from './mcpContract.helpers.js';

const READ_PDF_REQUIRED_FIELDS: Record<string, string[]> = {
  'meta-pages': ['info', 'num_pages'],
  'full-text': ['full_text'],
  markdown: ['markdown'],
  html: ['html'],
  chunks: ['chunks'],
  elements: ['elements'],
  'text-layer': ['text_layer'],
  tables: ['table_info'],
  'document-map': ['document_map'],
  'document-ast': ['document_ast'],
  safety: ['safety_findings'],
  layout: ['layout_diagnostics'],
  trust: ['trust_report'],
  a11y: ['accessibility_report'],
  'page-geometry': ['page_geometry'],
  structure: ['structure_trees'],
};

const READ_PDF_CASES: Array<{
  id: string;
  args: Record<string, unknown>;
  fields: string[];
}> = Object.entries(READ_PDF_REQUIRED_FIELDS).map(([id, fields]) => {
  const flagMap: Record<string, Record<string, unknown>> = {
    'meta-pages': { include_metadata: true, include_page_count: true },
    'full-text': { include_full_text: true },
    markdown: { include_markdown: true },
    html: { include_html: true },
    chunks: { include_chunks: true },
    elements: { include_elements: true, include_semantic_hints: true },
    'text-layer': { include_text_layer: true },
    tables: { include_tables: true },
    'document-map': { include_document_map: true },
    'document-ast': { include_document_ast: true },
    safety: { include_safety_findings: true },
    layout: { include_layout_diagnostics: true },
    trust: { include_trust_report: true },
    a11y: { include_accessibility_report: true },
    outline: { include_outline: true },
    annotations: { include_annotations: true },
    'page-labels': { include_page_labels: true },
    'page-geometry': { include_page_geometry: true },
    permissions: { include_permissions: true },
    forms: { include_form_fields: true },
    attachments: { include_attachments: true },
    structure: { include_structure_tree: true },
    images: { include_images: true },
    ocr: { include_ocr_text_layer: true },
  };
  return {
    id,
    args: { auto: false, ...flagMap[id] },
    fields,
  };
});

function deepHasKey(value: unknown, key: string): boolean {
  if (value === null || value === undefined) return false;
  if (Array.isArray(value)) return value.some((entry) => deepHasKey(entry, key));
  if (typeof value === 'object') {
    const record = value as Record<string, unknown>;
    if (Object.hasOwn(record, key)) return true;
    return Object.values(record).some((entry) => deepHasKey(entry, key));
  }
  return false;
}

// Sole-Rust is the production path; suite is on by default.
// Set RUN_PURE_RUST_CAPABILITY=0 to skip in constrained environments.
const pureRustEnabled = process.env.RUN_PURE_RUST_CAPABILITY !== '0';
const signalPdf = join(import.meta.dir, '../fixtures/differential/v3014-behavior-v1.pdf');
const structurePdf = join(import.meta.dir, '../fixtures/differential/v3014-structure-v1.pdf');
const selectableTablePdf = join(
  import.meta.dir,
  '../fixtures/differential/v3014-selectable-table-v1.pdf'
);
const rasterImagePdf = join(import.meta.dir, '../fixtures/differential/v3014-raster-images-v1.pdf');
const visualCandidatePdf = join(
  import.meta.dir,
  '../fixtures/differential/v3014-visual-candidate-v1.pdf'
);

describe.skipIf(!pureRustEnabled)('sole-Rust production capability contract', () => {
  let proc: ChildProcess;
  let reqId = 100;

  const nextId = () => {
    reqId += 1;
    return reqId;
  };

  beforeAll(async () => {
    process.env.PDF_READER_ENGINE_MODE = 'pure-rust';
    ensureProductionArtifacts();
    proc = spawnProductionMcp({
      PDF_READER_ENGINE_MODE: 'pure-rust',
      MCP_PDF_OCR_COMMAND: '',
      MCP_PDF_OCR_ARGS_JSON: '',
      MCP_PDF_OCR_PRESET: '',
      MCP_PDF_REGION_ANALYSIS_COMMAND: '',
      MCP_PDF_REGION_ANALYSIS_HTTP_URL: '',
      MCP_PDF_REGION_ANALYSIS_PRESET: '',
    });
    await initializeSession(proc, 'capability-parity-contract');
  }, 420_000);

  afterAll(() => {
    proc?.kill('SIGTERM');
  });

  test('every claimed non-optional read_pdf include_* capability returns its response field', async () => {
    const failures: string[] = [];
    for (const entry of READ_PDF_CASES) {
      const response = await callTool(
        proc,
        nextId(),
        'read_pdf',
        {
          sources: [
            {
              path:
                entry.id === 'structure'
                  ? structurePdf
                  : entry.id === 'tables'
                    ? selectableTablePdf
                    : samplePdf,
            },
          ],
          ...entry.args,
        },
        90_000
      );
      const payload = parseToolPayload(response);
      if (payload.isError) {
        failures.push(`${entry.id}: tool error: ${payload.text.slice(0, 240)}`);
        continue;
      }
      let parsed: unknown;
      try {
        parsed = JSON.parse(payload.text);
      } catch {
        failures.push(`${entry.id}: non-JSON payload`);
        continue;
      }
      for (const field of entry.fields) {
        if (!deepHasKey(parsed, field)) {
          failures.push(`${entry.id}: missing field '${field}'`);
        }
      }
    }
    expect(failures).toEqual([]);
  }, 600_000);

  test('raster image capability emits public metadata and PNG content without leaking binary data', async () => {
    const response = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: rasterImagePdf, pages: [1] }],
        auto: false,
        include_page_count: true,
        include_images: true,
      },
      90_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(false);
    const data = (
      JSON.parse(payload.text) as {
        results?: Array<{
          data?: {
            image_info?: Array<Record<string, unknown>>;
            images?: unknown;
          };
        }>;
      }
    ).results?.[0]?.data;
    expect(data?.image_info).toEqual([{ page: 1, index: 0, width: 2, height: 2, format: 'rgb' }]);
    expect(data?.images).toBeUndefined();
    expect(data?.image_info?.[0]?.data).toBeUndefined();

    const imageParts = response.result?.content?.filter((part) => part.type === 'image') ?? [];
    expect(imageParts).toHaveLength(1);
    expect(imageParts[0]?.mimeType).toBe('image/png');
    expect(imageParts[0]?.data).toMatch(/^[A-Za-z0-9+/]+={0,2}$/);
  }, 180_000);

  test('visual candidate capability retains provider-independent regions and omits unavailable enrichments', async () => {
    const response = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: visualCandidatePdf, pages: [1] }],
        auto: false,
        include_page_count: true,
        include_visual_enrichments: true,
      },
      90_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(false);
    const data = (
      JSON.parse(payload.text) as {
        results?: Array<{ data?: Record<string, unknown> }>;
      }
    ).results?.[0]?.data;

    expect(data?.visual_enrichment_candidates).toEqual([
      {
        id: 'p1-table-1',
        page: 1,
        region: {
          id: 'p1-table-1',
          page: 1,
          bounding_box: { left: 72, bottom: 665, right: 282.672, top: 702 },
        },
        target_element_id: 'p1-table-1',
        target_element_type: 'table',
        source_element_id: 'p1-table-1',
        candidate_signals: ['table-element', 'element-bounding-box'],
      },
    ]);
    expect(data?.visual_enrichments).toBeUndefined();
    expect(data?.elements).toBeUndefined();
    expect(data?.tables).toBeUndefined();
    expect(data?.page_geometry).toBeUndefined();
    expect(data?.warnings).toEqual([
      'Visual enrichment skipped: analyze_regions provider is not_configured.',
    ]);
  }, 180_000);

  test('structure trees match the immutable tagged subset and untagged PDFs omit the field', async () => {
    const tagged = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: structurePdf, pages: [2, 1, 1] }],
        auto: false,
        include_structure_tree: true,
      },
      90_000
    );
    const taggedData = (
      JSON.parse(parseToolPayload(tagged).text) as {
        results?: Array<{ data?: Record<string, unknown> }>;
      }
    ).results?.[0]?.data;
    expect(taggedData?.structure_trees).toEqual([
      {
        page: 1,
        tree: {
          role: 'Root',
          children: [
            { role: 'H1', children: [{ type: 'content', id: 'p3R_mc0' }] },
            {
              role: 'Figure',
              children: [{ type: 'annotation', id: 'pdfjs_internal_id_7R' }],
            },
          ],
        },
      },
      { page: 2, tree: { role: 'Root' } },
    ]);

    const untagged = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: samplePdf }],
        auto: false,
        include_structure_tree: true,
      },
      90_000
    );
    const untaggedData = (
      JSON.parse(parseToolPayload(untagged).text) as {
        results?: Array<{ data?: Record<string, unknown> }>;
      }
    ).results?.[0]?.data;
    expect(untaggedData?.structure_trees).toBeUndefined();
  }, 180_000);

  test('annotations match the immutable Link subset and omit empty placeholders', async () => {
    const withAnnotation = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: signalPdf, pages: [1] }],
        auto: false,
        include_annotations: true,
      },
      90_000
    );
    const populated = JSON.parse(parseToolPayload(withAnnotation).text) as {
      results?: Array<{ data?: { annotations?: unknown } }>;
    };
    expect(populated.results?.[0]?.data?.annotations).toEqual([
      {
        page: 1,
        annotations: [
          {
            page: 1,
            id: '11R',
            subtype: 'Link',
            contents: '  Linked note  ',
            url: 'https://example.com/a',
            bounding_box: { left: 50, bottom: 150, right: 100, top: 200 },
          },
        ],
      },
    ]);

    const withoutAnnotation = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: samplePdf }],
        auto: false,
        include_annotations: true,
      },
      90_000
    );
    const omitted = JSON.parse(parseToolPayload(withoutAnnotation).text) as {
      results?: Array<{ data?: { annotations?: unknown } }>;
    };
    expect(omitted.results?.[0]?.data?.annotations).toBeUndefined();
  }, 180_000);

  test('forms and attachments match separate immutable fixtures and omit placeholders', async () => {
    const formsResponse = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: signalPdf }],
        auto: false,
        include_form_fields: true,
      },
      90_000
    );
    const formsData = (
      JSON.parse(parseToolPayload(formsResponse).text) as {
        results?: Array<{ data?: Record<string, unknown> }>;
      }
    ).results?.[0]?.data;
    expect(formsData?.form_fields).toEqual([
      {
        name: 'customer_name',
        type: 'text',
        value: 'Ada Lovelace',
        default_value: '',
        page: 1,
        id: '22R',
        editable: true,
        bounding_box: { left: 72, bottom: 635, right: 260, top: 660 },
      },
      { name: 'profile', id: '24R' },
      {
        name: 'profile',
        type: 'text',
        value: 'Grace Hopper',
        default_value: 'Unknown',
        page: 2,
        id: '25R',
        editable: false,
        bounding_box: { left: 72, bottom: 500, right: 260, top: 525 },
      },
      {
        name: 'consent',
        type: 'checkbox',
        value: 'Yes',
        default_value: null,
        page: 2,
        id: '26R',
        editable: true,
        bounding_box: { left: 72, bottom: 450, right: 90, top: 468 },
      },
      {
        name: 'tier',
        type: 'listbox',
        value: 'gold',
        default_value: 'silver',
        page: 3,
        id: '27R',
        editable: true,
        bounding_box: { left: 72, bottom: 400, right: 200, top: 425 },
      },
    ]);
    expect(formsData?.attachments).toBeUndefined();

    const attachmentsResponse = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: signalPdf }],
        auto: false,
        include_attachments: true,
      },
      90_000
    );
    const attachmentsData = (
      JSON.parse(parseToolPayload(attachmentsResponse).text) as {
        results?: Array<{ data?: Record<string, unknown> }>;
      }
    ).results?.[0]?.data;
    expect(attachmentsData?.attachments).toEqual([
      {
        name: 'source.csv',
        filename: 'source.csv',
        description: 'Source data',
        size_bytes: 19,
      },
      { name: 'evidence', filename: 'report.txt', size_bytes: 5 },
    ]);
    expect(attachmentsData?.form_fields).toBeUndefined();

    const emptyResponse = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: samplePdf }],
        auto: false,
        include_form_fields: true,
        include_attachments: true,
      },
      90_000
    );
    const emptyData = (
      JSON.parse(parseToolPayload(emptyResponse).text) as {
        results?: Array<{ data?: Record<string, unknown> }>;
      }
    ).results?.[0]?.data;
    expect(emptyData?.form_fields).toBeUndefined();
    expect(emptyData?.attachments).toBeUndefined();
  }, 180_000);

  test('catalog signals match the immutable v3.0.14 subset and omit unavailable values', async () => {
    const outlineResponse = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: signalPdf }],
        auto: false,
        include_outline: true,
      },
      90_000
    );
    const outlineData = (
      JSON.parse(parseToolPayload(outlineResponse).text) as {
        results?: Array<{ data?: Record<string, unknown> }>;
      }
    ).results?.[0]?.data;
    expect(outlineData?.outline).toEqual([
      {
        title: 'External docs',
        bold: true,
        italic: true,
        color: [64, 128, 191],
        url: 'https://example.com/docs',
        dest: null,
        items: [
          {
            title: 'Page three',
            bold: false,
            italic: false,
            color: [0, 0, 0],
            dest: [{ num: 7, gen: 0 }, { name: 'Fit' }],
          },
        ],
      },
    ]);
    expect(outlineData?.page_labels).toBeUndefined();
    expect(outlineData?.permissions).toBeUndefined();
    expect(outlineData?.mark_info).toBeUndefined();

    const pageLabelsResponse = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: signalPdf }],
        auto: false,
        include_page_labels: true,
      },
      90_000
    );
    const pageLabelsData = (
      JSON.parse(parseToolPayload(pageLabelsResponse).text) as {
        results?: Array<{ data?: Record<string, unknown> }>;
      }
    ).results?.[0]?.data;
    expect(pageLabelsData?.page_labels).toEqual(['P-1', 'iv', 'AA']);
    expect(pageLabelsData?.outline).toBeUndefined();
    expect(pageLabelsData?.permissions).toBeUndefined();
    expect(pageLabelsData?.mark_info).toBeUndefined();

    const permissionsResponse = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: signalPdf }],
        auto: false,
        include_permissions: true,
      },
      90_000
    );
    const permissionsData = (
      JSON.parse(parseToolPayload(permissionsResponse).text) as {
        results?: Array<{ data?: Record<string, unknown> }>;
      }
    ).results?.[0]?.data;
    expect(permissionsData?.permissions).toBeUndefined();
    expect(permissionsData?.mark_info).toEqual({
      Marked: true,
      UserProperties: true,
      Suspects: false,
    });
    expect(permissionsData?.outline).toBeUndefined();
    expect(permissionsData?.page_labels).toBeUndefined();

    const withoutSignals = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: samplePdf }],
        auto: false,
        include_outline: true,
        include_page_labels: true,
        include_permissions: true,
      },
      90_000
    );
    const omitted = JSON.parse(parseToolPayload(withoutSignals).text) as {
      results?: Array<{
        data?: {
          outline?: unknown;
          page_labels?: unknown;
          permissions?: unknown;
          mark_info?: unknown;
        };
      }>;
    };
    expect(omitted.results?.[0]?.data?.outline).toBeUndefined();
    expect(omitted.results?.[0]?.data?.page_labels).toBeUndefined();
    expect(omitted.results?.[0]?.data?.permissions).toBeUndefined();
    expect(omitted.results?.[0]?.data?.mark_info).toBeUndefined();
  }, 180_000);

  test('auto balanced twin matches v3.0.14 depth without full text', async () => {
    const response = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: samplePdf }],
        auto: true,
        auto_detail: 'balanced',
      },
      90_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(false);
    const parsed = JSON.parse(payload.text) as unknown;
    const missing: string[] = [];
    for (const field of [
      'markdown',
      'chunks',
      'document_map',
      'trust_report',
      'accessibility_report',
    ]) {
      if (!deepHasKey(parsed, field)) missing.push(field);
    }
    expect(missing).toEqual([]);
    expect(deepHasKey(parsed, 'full_text')).toBe(false);
  }, 120_000);

  test('provider-backed OCR is omitted with an explicit warning when no provider is configured', async () => {
    const response = await callTool(
      proc,
      nextId(),
      'read_pdf',
      {
        sources: [{ path: samplePdf, pages: [1] }],
        auto: false,
        include_ocr_text_layer: true,
      },
      90_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(false);
    const parsed = JSON.parse(payload.text) as {
      results?: Array<{
        success?: boolean;
        data?: { ocr_text_layer?: unknown; warnings?: string[] };
      }>;
    };
    expect(parsed.results?.[0]?.success).toBe(true);
    expect(parsed.results?.[0]?.data?.ocr_text_layer).toBeUndefined();
    expect(parsed.results?.[0]?.data?.warnings).toContain(
      'OCR text layer unavailable: OCR provider is not configured. Set MCP_PDF_OCR_COMMAND or MCP_PDF_OCR_PRESET=tesseract to enable ocr_pages.'
    );
  }, 120_000);

  test('search_pdf public options remain available', async () => {
    for (const args of [
      { query: 'a', case_sensitive: true },
      { query: 'a', whole_word: true },
      { query: 'a', context_chars: 40, max_matches_per_source: 5 },
    ]) {
      const response = await callTool(
        proc,
        nextId(),
        'search_pdf',
        { sources: [{ path: samplePdf }], ...args, detail: true },
        60_000
      );
      const payload = parseToolPayload(response);
      expect(payload.isError).toBe(false);
      const parsed = JSON.parse(payload.text) as unknown;
      expect(deepHasKey(parsed, 'matches') || deepHasKey(parsed, 'results')).toBe(true);
    }
  }, 180_000);

  test('pdf_evidence inspect and bounded visual ops work; provider ops fail closed', async () => {
    const inspect = await callTool(
      proc,
      nextId(),
      'pdf_evidence',
      {
        operation: 'inspect',
        sources: [{ path: samplePdf }],
        sample_pages: 2,
      },
      60_000
    );
    const inspectPayload = parseToolPayload(inspect);
    expect(inspectPayload.isError).toBe(false);
    const inspectJson = JSON.parse(inspectPayload.text) as unknown;
    expect(deepHasKey(inspectJson, 'recommendation') || deepHasKey(inspectJson, 'num_pages')).toBe(
      true
    );

    const taggedInspect = await callTool(
      proc,
      nextId(),
      'pdf_evidence',
      {
        operation: 'inspect',
        sources: [{ path: structurePdf }],
        sample_pages: 2,
      },
      60_000
    );
    const taggedInspectJson = JSON.parse(parseToolPayload(taggedInspect).text) as {
      results?: Array<{
        data?: {
          profile?: string;
          sampled_pages?: number[];
          page_signals?: unknown[];
          document_signals?: Record<string, boolean>;
          recommendation?: {
            workflow?: string;
            needs_ocr?: boolean;
            read_pdf_arguments?: Record<string, unknown>;
          };
        };
      }>;
    };
    expect(taggedInspectJson.results?.[0]?.data?.profile).toBe('low_text_or_form');
    expect(taggedInspectJson.results?.[0]?.data?.sampled_pages).toEqual([1, 2]);
    expect(taggedInspectJson.results?.[0]?.data?.page_signals).toEqual([
      {
        page: 1,
        text_chars: 14,
        text_items: 1,
        estimated_tokens: 4,
        image_paint_operations: 0,
        likely_scanned: false,
        low_text_density: true,
      },
      {
        page: 2,
        text_chars: 0,
        text_items: 0,
        estimated_tokens: 0,
        image_paint_operations: 0,
        likely_scanned: false,
        low_text_density: true,
      },
    ]);
    expect(taggedInspectJson.results?.[0]?.data?.document_signals).toEqual({
      has_outline: false,
      has_page_labels: false,
      has_permissions: false,
      has_mark_info: true,
      has_form_fields: false,
      has_attachments: false,
      has_structure_tree: true,
    });
    expect(taggedInspectJson.results?.[0]?.data?.recommendation).toMatchObject({
      workflow: 'metadata_review',
      needs_ocr: false,
      read_pdf_arguments: {
        sources: [{ path: structurePdf }],
        include_metadata: true,
        include_page_count: true,
        include_page_geometry: true,
        include_structure_tree: true,
      },
    });

    for (const operation of ['render_page', 'extract_regions']) {
      const response = await callTool(
        proc,
        nextId(),
        'pdf_evidence',
        {
          operation,
          sources: [
            {
              path: samplePdf,
              regions:
                operation === 'extract_regions' || operation === 'analyze_regions'
                  ? [
                      {
                        id: 'r1',
                        page: 1,
                        bounding_box: {
                          left: 0,
                          bottom: 0,
                          right: 100,
                          top: 100,
                        },
                      },
                    ]
                  : undefined,
            },
          ],
          max_pages: 1,
          max_regions: 1,
        },
        60_000
      );
      const payload = parseToolPayload(response);
      expect(payload.isError).toBe(false);
      const result = JSON.parse(payload.text) as unknown;
      expect(deepHasKey(result, operation === 'render_page' ? 'rendered_pages' : 'regions')).toBe(
        true
      );
    }

    for (const operation of ['ocr_pages', 'analyze_regions']) {
      const response = await callTool(
        proc,
        nextId(),
        'pdf_evidence',
        {
          operation,
          sources: [
            {
              path: samplePdf,
              regions:
                operation === 'analyze_regions'
                  ? [
                      {
                        id: 'r1',
                        page: 1,
                        bounding_box: {
                          left: 0,
                          bottom: 0,
                          right: 100,
                          top: 100,
                        },
                      },
                    ]
                  : undefined,
            },
          ],
          max_pages: 1,
          max_regions: 1,
        },
        60_000
      );
      const payload = parseToolPayload(response);
      expect(payload.isError).toBe(true);
      expect(payload.text).toContain(
        operation === 'ocr_pages'
          ? 'OCR provider is not configured'
          : 'Region analysis provider is not configured'
      );
    }
  }, 240_000);
});
