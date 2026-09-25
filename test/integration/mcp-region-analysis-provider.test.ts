import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import type { ChildProcess } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
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
const provider = path.join(
  repoRoot,
  'test/fixtures/providers/reference-region-analysis-provider.ts'
);
const providerArgs = (mode?: string, marker?: string) =>
  JSON.stringify([
    provider,
    '{input}',
    '{page}',
    '{region_id}',
    '{evidence_id}',
    '{languages}',
    ...(mode ? [mode] : []),
    ...(marker ? [marker] : []),
  ]);

describe('pure-Rust command region analysis provider integration', () => {
  let proc: ChildProcess;
  let failingProc: ChildProcess;
  let timeoutProc: ChildProcess;
  let descendantProc: ChildProcess;
  let aggregateProc: ChildProcess;
  const workspace = mkdtempSync(path.join(tmpdir(), 'pdf-reader-analyze-provider-'));
  const descendantMarker = path.join(workspace, 'descendant-input.txt');
  const invocationMarker = path.join(workspace, 'aggregate-invocations.txt');

  beforeAll(async () => {
    ensureProductionArtifacts();
    proc = spawnProductionMcp({
      PDF_READER_ENGINE_MODE: 'pure-rust',
      MCP_PDF_REGION_ANALYSIS_COMMAND: process.execPath,
      MCP_PDF_REGION_ANALYSIS_ARGS_JSON: providerArgs(),
    });
    await initializeSession(proc, 'rust-region-analysis-provider-contract');
    failingProc = spawnProductionMcp({
      PDF_READER_ENGINE_MODE: 'pure-rust',
      MCP_PDF_REGION_ANALYSIS_COMMAND: process.execPath,
      MCP_PDF_REGION_ANALYSIS_ARGS_JSON: providerArgs('fail'),
    });
    await initializeSession(failingProc, 'rust-region-analysis-provider-failure-contract');
    timeoutProc = spawnProductionMcp({
      PDF_READER_ENGINE_MODE: 'pure-rust',
      MCP_PDF_REGION_ANALYSIS_COMMAND: process.execPath,
      MCP_PDF_REGION_ANALYSIS_ARGS_JSON: providerArgs('sleep'),
    });
    await initializeSession(timeoutProc, 'rust-region-analysis-provider-timeout-contract');
    descendantProc = spawnProductionMcp({
      PDF_READER_ENGINE_MODE: 'pure-rust',
      MCP_PDF_REGION_ANALYSIS_COMMAND: process.execPath,
      MCP_PDF_REGION_ANALYSIS_ARGS_JSON: providerArgs('escaped-descendant', descendantMarker),
    });
    await initializeSession(descendantProc, 'rust-region-analysis-provider-descendant-contract');
    aggregateProc = spawnProductionMcp({
      PDF_READER_ENGINE_MODE: 'pure-rust',
      MCP_PDF_REGION_ANALYSIS_COMMAND: process.execPath,
      MCP_PDF_REGION_ANALYSIS_ARGS_JSON: providerArgs('oversize', invocationMarker),
    });
    await initializeSession(aggregateProc, 'rust-region-analysis-provider-aggregate-contract');
  }, 420_000);

  afterAll(() => {
    proc?.kill('SIGTERM');
    failingProc?.kill('SIGTERM');
    timeoutProc?.kill('SIGTERM');
    descendantProc?.kill('SIGTERM');
    aggregateProc?.kill('SIGTERM');
    rmSync(workspace, { recursive: true, force: true });
  });

  test('crops a bounded region and returns exact rich normalized provider data', async () => {
    const response = await callTool(
      proc,
      51,
      'pdf_evidence',
      {
        operation: 'analyze_regions',
        sources: [
          {
            path: fixture,
            regions: [
              {
                id: 'rich',
                page: 1,
                bounding_box: { left: 0, bottom: 0, right: 30, top: 20 },
              },
            ],
          },
        ],
        scale: 2,
        max_regions: 1,
        languages: ['fra', 'eng'],
      },
      90_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(false);
    expect(response.result?.content).toHaveLength(1);
    const result = JSON.parse(payload.text) as {
      profile: string;
      results: Array<{ region_analyses: Array<Record<string, unknown>> }>;
    };
    expect(result.profile).toBe('region_analysis');
    expect(result.results[0]?.region_analyses[0]).toMatchObject({
      region_id: 'rich',
      page: 1,
      kind: 'unknown',
      description: 'region rich languages fra,eng',
      confidence: 0.87,
      provider: 'command',
      source_crop_evidence_id: 'page-1-rich-crop-scale-2',
      crop_pixels: { left: 0, top: 120, width: 60, height: 40 },
      provenance: { engine: 'external-command', source: 'region-analysis-provider' },
      table: { row_count: 2, column_count: 4, confidence: 0.92 },
      formula: { asciimath: 'x^2', confidence: 1 },
      chart: { title: 'Sales', confidence: 0.88 },
    });
  }, 120_000);

  test('fails closed when the command provider exits unsuccessfully', async () => {
    const response = await callTool(
      failingProc,
      52,
      'pdf_evidence',
      {
        operation: 'analyze_regions',
        sources: [
          {
            path: fixture,
            regions: [
              {
                id: 'rich',
                page: 1,
                bounding_box: { left: 0, bottom: 0, right: 30, top: 20 },
              },
            ],
          },
        ],
        scale: 1,
        max_regions: 1,
      },
      30_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(true);
    expect(payload.text).toContain('All PDF sources failed region analysis');
    expect(payload.text).toContain('command failed for page 1 region rich');
  });

  test('bounds provider timeout', async () => {
    const started = Date.now();
    const response = await callTool(
      timeoutProc,
      53,
      'pdf_evidence',
      {
        operation: 'analyze_regions',
        sources: [
          {
            path: fixture,
            regions: [
              {
                id: 'rich',
                page: 1,
                bounding_box: { left: 0, bottom: 0, right: 30, top: 20 },
              },
            ],
          },
        ],
        scale: 1,
        max_regions: 1,
        timeout_ms: 1_000,
      },
      30_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(true);
    expect(payload.text).toContain('command timed out for page 1 region rich');
    expect(Date.now() - started).toBeLessThan(4_000);
  }, 30_000);

  test('bounds escaped inherited-pipe descendants and cleans the temporary crop', async () => {
    const started = Date.now();
    const response = await callTool(
      descendantProc,
      54,
      'pdf_evidence',
      {
        operation: 'analyze_regions',
        sources: [
          {
            path: fixture,
            regions: [
              {
                id: 'rich',
                page: 1,
                bounding_box: { left: 0, bottom: 0, right: 30, top: 20 },
              },
            ],
          },
        ],
        scale: 1,
        max_regions: 1,
        timeout_ms: 1_000,
      },
      30_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(true);
    expect(payload.text).toContain('command timed out for page 1 region rich');
    expect(Date.now() - started).toBeLessThan(4_000);
    const temporaryInput = readFileSync(descendantMarker, 'utf8');
    expect(existsSync(temporaryInput)).toBe(false);
  }, 30_000);

  test('charges failed oversized stdout, keeps exhaustion sticky, and preserves early success', async () => {
    const regionSource = (id: string) => ({
      path: fixture,
      regions: [
        {
          id,
          page: 1,
          bounding_box: { left: 0, bottom: 0, right: 30, top: 20 },
        },
      ],
    });
    const response = await callTool(
      aggregateProc,
      55,
      'pdf_evidence',
      {
        operation: 'analyze_regions',
        sources: [
          regionSource('rich-first'),
          ...Array.from({ length: 16 }, (_, index) => regionSource(`oversize-${index + 1}`)),
          regionSource('must-not-run'),
        ],
        scale: 1,
        max_regions: 1,
        max_output_chars: 1_000,
      },
      120_000
    );
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(false);
    const result = JSON.parse(payload.text) as {
      results: Array<{
        success: boolean;
        region_analyses?: Array<{ region_id: string }>;
        error?: string;
      }>;
    };
    expect(result.results[0]).toMatchObject({
      success: true,
      region_analyses: [{ region_id: 'rich-first' }],
    });
    expect(result.results.at(-1)).toMatchObject({
      success: false,
      error: expect.stringContaining('Request exceeds region analysis provider output limit'),
    });
    const invocations = readFileSync(invocationMarker, 'utf8').trim().split('\n');
    expect(invocations).toHaveLength(17);
    expect(invocations[0]).toBe('rich-first');
    expect(invocations).not.toContain('must-not-run');
  }, 120_000);
});
