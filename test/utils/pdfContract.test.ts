import { describe, expect, test } from 'bun:test';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { JsonRpcResponse } from '../production/mcpContract.helpers.js';
import { assertPdfSuccess, requirePdfFixture } from './pdfContract.js';

const response = (payload: unknown): JsonRpcResponse => ({
  result: { content: [{ type: 'text', text: JSON.stringify(payload) }] },
});

describe('PDF fixture contracts without native processes', () => {
  test('checks fixture availability before invoking a tool', () => {
    const root = mkdtempSync(join(tmpdir(), 'anymd-pdf-contract-'));
    let calls = 0;
    const invoke = (file: string) => {
      requirePdfFixture(file);
      calls += 1;
    };
    try {
      expect(() => invoke(join(root, 'missing.pdf'))).toThrow();
      expect(() => invoke(root)).toThrow('not a regular file');
      expect(calls).toBe(0);
      const file = join(root, 'sample.pdf');
      writeFileSync(file, '%PDF-1.4');
      invoke(file);
      expect(calls).toBe(1);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  test('accepts successful text and structured source results', () => {
    const payload = { results: [{ success: true, data: { num_pages: 1 } }] };
    assertPdfSuccess(response(payload));
    assertPdfSuccess({ result: { structuredContent: payload } });
  });

  test('rejects decoder and operation regressions, even with nonempty PDF errors', () => {
    const errors: JsonRpcResponse[] = [
      { error: { message: 'PDF decoder failed' } },
      { error: { message: 'Invalid pdf_evidence arguments' } },
      { result: { isError: true, content: [{ text: 'PDF render failed' }] } },
      response({ results: [{ success: false, error: 'PDF decoder failed' }] }),
      response({ results: [{ success: true }, { success: false, error: 'region failed' }] }),
      response({ results: [] }),
      response({ results: [{ data: {} }] }),
      response({ message: 'OCR provider is not configured' }),
      { result: { content: [{ text: 'nonempty operation error' }] } },
      {},
    ];
    for (const error of errors) {
      expect(() => assertPdfSuccess(error)).toThrow();
    }
  });
});
