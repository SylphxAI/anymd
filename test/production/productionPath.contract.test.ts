/**
 * Star-project production contract suite.
 *
 * Sole-Rust production path: dist/runtime-entry.js + platform native binary (ADR-0006).
 * TypeScript remains oracle/test-only and is not the production package entry.
 */

import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import type { ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import {
  callTool,
  ensureProductionArtifacts,
  initializeSession,
  listTools,
  packageJson,
  parseToolPayload,
  productionEnv,
  repoRoot,
  samplePdf,
  spawnProductionMcp,
} from './mcpContract.helpers.js';

type ContractMatrix = {
  requiredTools: string[];
  readPdfOptionSmoke: Array<{ id: string; args: Record<string, unknown> }>;
  searchPdfCases: Array<{ id: string; args: Record<string, unknown> }>;
  pdfEvidenceCases: Array<{ id: string; args: Record<string, unknown> }>;
  securityCases: Array<{
    id: string;
    url?: string;
    pathAndUrl?: boolean;
  }>;
};

const matrix = JSON.parse(
  fs.readFileSync(path.join(import.meta.dirname, 'publicContract.matrix.json'), 'utf8')
) as ContractMatrix;

const buildReadPdfArgs = (entry: { id: string; args: Record<string, unknown> }) => {
  const args = { ...entry.args };
  const pageSpec = args.sources_pages;
  args.sources_pages = undefined;
  if (pageSpec !== undefined) {
    args.sources = [{ path: samplePdf, pages: pageSpec }];
  } else if (!args.sources) {
    args.sources = [{ path: samplePdf }];
  }
  return args as Record<string, unknown>;
};

describe('production-path public contract (sole-Rust production entry)', () => {
  let proc: ChildProcess;
  let reqId = 10;

  const nextId = () => {
    reqId += 1;
    return reqId;
  };

  beforeAll(() => {
    ensureProductionArtifacts();
    expect(productionEnv().PDF_READER_ENGINE_MODE).toBeUndefined();

    proc = spawnProductionMcp();
  }, 420_000);

  afterAll(() => {
    proc?.kill('SIGTERM');
  });

  test('package public entry is sole-Rust runtime-entry without typescript export', () => {
    expect(packageJson.bin?.anymd).toBe('./dist/runtime-entry.js');
    expect(packageJson.exports?.['.']).toBe('./dist/runtime-entry.js');
    expect(packageJson.exports?.['./typescript']).toBeUndefined();
    expect(packageJson.files).toContain('dist/runtime-entry.js');
    expect(packageJson.files).toContain('dist/pure-rust.js');
    expect(packageJson.files).not.toContain('dist/');
    expect(packageJson.version).toMatch(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/);
    expect(fs.existsSync(path.join(repoRoot, 'dist/runtime-entry.js'))).toBe(true);
  });

  test('initialize advertises anymd and package version', async () => {
    const init = await initializeSession(proc, 'production-contract-suite');
    expect(init.result?.serverInfo?.name).toBe('anymd');
    expect(init.result?.serverInfo?.version).toBe(packageJson.version);
  }, 60_000);

  test('tools/list exposes exactly the public V3 tool surface', async () => {
    const listed = await listTools(proc, nextId());
    const names = (listed.result?.tools ?? []).map((t) => t.name).sort();
    for (const tool of matrix.requiredTools) {
      expect(names).toContain(tool);
    }
    expect(names).not.toContain('inspect_pdf');
    expect(names).not.toContain('render_page');
    expect(names).not.toContain('extract_regions');
    expect(names).not.toContain('ocr_pages');
    expect(names).not.toContain('analyze_regions');
    // Schema must be typed objects, not empty Value blobs
    for (const tool of listed.result?.tools ?? []) {
      if (matrix.requiredTools.includes(tool.name)) {
        const schema = tool.inputSchema as { type?: string; properties?: Record<string, unknown> };
        expect(schema?.type === 'object' || schema?.properties).toBeTruthy();
      }
    }
  }, 30_000);

  test('read_pdf option smoke matrix succeeds on production path', async () => {
    const failures: string[] = [];
    for (const entry of matrix.readPdfOptionSmoke) {
      const response = await callTool(proc, nextId(), 'read_pdf', buildReadPdfArgs(entry), 90_000);
      const payload = parseToolPayload(response);
      if (payload.isError) {
        failures.push(`${entry.id}: ${payload.text.slice(0, 300)}`);
        continue;
      }
      const text = payload.text.toLowerCase();
      const ok =
        text.includes('success') ||
        text.includes('results') ||
        text.includes('document') ||
        text.includes('num_pages') ||
        text.includes('markdown') ||
        text.includes('elements') ||
        text.includes('page');
      if (!ok) {
        failures.push(`${entry.id}: unexpected payload shape: ${payload.text.slice(0, 300)}`);
      }
    }
    expect(failures).toEqual([]);
  }, 600_000);

  test('search_pdf public option smoke succeeds on production path', async () => {
    const failures: string[] = [];
    for (const entry of matrix.searchPdfCases) {
      const response = await callTool(
        proc,
        nextId(),
        'search_pdf',
        {
          sources: [{ path: samplePdf }],
          ...entry.args,
        },
        90_000
      );
      const payload = parseToolPayload(response);
      if (payload.isError) {
        failures.push(`${entry.id}: ${payload.text.slice(0, 300)}`);
      }
    }
    expect(failures).toEqual([]);
  }, 300_000);

  test('pdf_evidence public operations smoke on production path (sole-Rust)', async () => {
    const failures: string[] = [];
    for (const entry of matrix.pdfEvidenceCases) {
      const args = structuredClone(entry.args) as Record<string, unknown>;
      if (!args.sources) {
        args.sources = [{ path: samplePdf }];
      } else if (Array.isArray(args.sources)) {
        args.sources = args.sources.map((source) => {
          if (!source || typeof source !== 'object') return source;
          const entrySource = { ...(source as Record<string, unknown>) };
          if (entrySource.path === '__SAMPLE__') entrySource.path = samplePdf;
          return entrySource;
        });
      }
      const response = await callTool(proc, nextId(), 'pdf_evidence', args, 120_000);
      const payload = parseToolPayload(response);
      // TS path: ops may succeed or fail on missing optional canvas/OCR providers,
      // but must not crash the process. inspect must not be hard-error without source.
      if (payload.isError && entry.id === 'inspect') {
        failures.push(`${entry.id}: ${payload.text.slice(0, 400)}`);
      }
      expect(payload.text.length).toBeGreaterThan(0);
    }
    expect(failures).toEqual([]);
  }, 300_000);

  test('security contract rejects private/transition SSRF URLs and invalid locators', async () => {
    const failures: string[] = [];
    for (const entry of matrix.securityCases) {
      let args: Record<string, unknown>;
      if (entry.pathAndUrl) {
        args = {
          sources: [{ path: samplePdf, url: 'https://example.com/x.pdf' }],
          include_full_text: false,
          auto: false,
        };
      } else {
        args = {
          sources: [{ url: entry.url }],
          include_full_text: false,
          auto: false,
        };
      }
      const response = await callTool(proc, nextId(), 'read_pdf', args, 60_000);
      const payload = parseToolPayload(response);
      const text = payload.text.toLowerCase();
      const blocked =
        payload.isError ||
        text.includes('non-public') ||
        text.includes('ssrf') ||
        text.includes('exactly one') ||
        text.includes('failed') ||
        text.includes('private') ||
        text.includes('invalid') ||
        text.includes('blocked') ||
        text.includes('not allowed');
      if (!blocked) {
        failures.push(`${entry.id}: expected fail-closed, got: ${payload.text.slice(0, 300)}`);
      }
    }
    expect(failures).toEqual([]);
  }, 300_000);
});
