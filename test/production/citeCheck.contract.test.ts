import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import type { ChildProcess } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  callTool,
  ensureProductionArtifacts,
  fixturesRoot,
  initializeSession,
  listTools,
  parseToolPayload,
  spawnProductionMcp,
} from './mcpContract.helpers.js';

// License-free deterministic PDF with no model, OCR or font-file dependency.
const citationPdf = (): Buffer => {
  const stream = 'BT /F1 12 Tf 50 700 Td (Deterministic quote) Tj ET';
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
    `<< /Length ${Buffer.byteLength(stream)} >>\nstream\n${stream}\nendstream`,
  ];
  let output = '%PDF-1.4\n';
  const offsets = [0];
  for (const [index, object] of objects.entries()) {
    offsets.push(Buffer.byteLength(output));
    output += `${index + 1} 0 obj\n${object}\nendobj\n`;
  }
  const xref = Buffer.byteLength(output);
  output += `xref\n0 6\n0000000000 65535 f \n`;
  output += offsets
    .slice(1)
    .map((offset) => `${String(offset).padStart(10, '0')} 00000 n \n`)
    .join('');
  output += `trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(output);
};

type Result = {
  scope: string;
  source_hash: string;
  results: Array<{
    verdict: string;
    locations: Array<{ geometry_level: string; observed_text: string }>;
  }>;
};

describe('cite-check public inspect contract', () => {
  let proc: ChildProcess;
  let directory: string;
  let source: string;
  let hash: string;
  let requestId = 20;
  const boundingBox = { left: 0, bottom: 0, right: 612, top: 792 };

  beforeAll(async () => {
    ensureProductionArtifacts();
    directory = fs.mkdtempSync(path.join(os.tmpdir(), 'anymd-cite-check-'));
    source = path.join(directory, 'citation.pdf');
    const bytes = citationPdf();
    fs.writeFileSync(source, bytes);
    hash = createHash('sha256').update(bytes).digest('hex');
    proc = spawnProductionMcp();
    await initializeSession(proc, 'cite-check-contract');
  }, 420_000);

  afterAll(() => {
    proc?.kill('SIGTERM');
    if (directory) fs.rmSync(directory, { recursive: true, force: true });
  });

  const check = async (quote: string, options: Record<string, unknown> = {}): Promise<Result> => {
    const response = await callTool(proc, ++requestId, 'inspect', {
      operation: 'cite_check',
      sources: [{ path: source }],
      citations: [{ quote, page: 1, bounding_box: boundingBox }],
      ...options,
    });
    const payload = parseToolPayload(response);
    expect(payload.isError).toBe(false);
    return JSON.parse(payload.text) as Result;
  };

  test('four tools remain four and cite-check is an inspect operation', async () => {
    const tools = (await listTools(proc, ++requestId)).result?.tools ?? [];
    expect(tools.map((tool) => tool.name).sort()).toEqual(['inspect', 'outline', 'read', 'search']);
    const schema = JSON.stringify(tools.find((tool) => tool.name === 'inspect')?.inputSchema);
    expect(schema).toContain('cite_check');
    expect(schema).toContain('citations');
    expect(schema).toContain('whitespace_v1');
  });

  test('exact support retains source hash and estimated geometry', async () => {
    const result = await check('Deterministic quote', { expected_source_sha256: hash });
    expect(result.source_hash).toBe(hash);
    expect(result.scope).toContain('not semantic truth');
    expect(result.results[0].verdict).toBe('verified_exact');
    expect(result.results[0].locations[0].geometry_level).toBe('char_estimated');
  });

  test('normalization is explicit and does not fold case', async () => {
    expect((await check('Deterministic   quote')).results[0].verdict).toBe('unmatched');
    const normalized = await check('  Deterministic   quote  ', { normalization: 'whitespace_v1' });
    expect(normalized.results[0].verdict).toBe('verified_normalized');
    expect(normalized.results[0].locations[0].observed_text).toBe('Deterministic quote');
    expect(
      (await check('deterministic quote', { normalization: 'whitespace_v1' })).results[0].verdict
    ).toBe('unmatched');
  });

  test('wrong location is unmatched; hash mismatch and scan are insufficient', async () => {
    const wrong = await check('Deterministic quote', {
      citations: [
        {
          quote: 'Deterministic quote',
          page: 1,
          bounding_box: { left: 0, bottom: 0, right: 10, top: 10 },
        },
      ],
    });
    expect(wrong.results[0].verdict).toBe('unmatched');
    expect(
      (await check('Deterministic quote', { expected_source_sha256: '0'.repeat(64) })).results[0]
        .verdict
    ).toBe('insufficient_evidence');
    expect(
      (await check('quote', { sources: [{ path: path.join(fixturesRoot, 'scanned-page.pdf') }] }))
        .results[0].verdict
    ).toBe('insufficient_evidence');
  });

  test('runtime rejects UTF-16 and geometry bounds before work', async () => {
    for (const citations of [
      [{ quote: '😀'.repeat(2049), page: 1, bounding_box: boundingBox }],
      [{ quote: 'quote', page: 0, bounding_box: boundingBox }],
      [{ quote: 'quote', page: 1, bounding_box: { ...boundingBox, right: 0 } }],
    ]) {
      const response = await callTool(proc, ++requestId, 'inspect', {
        operation: 'cite_check',
        sources: [{ path: source }],
        citations,
      });
      expect(response.error?.code).toBe(-32602);
    }
  });
});
