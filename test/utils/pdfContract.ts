import assert from 'node:assert/strict';
import { statSync } from 'node:fs';
import type { JsonRpcResponse } from '../production/mcpContract.helpers.js';

// Check the prerequisite before the call, never infer availability from a tool error.
export const requirePdfFixture = (file: string): void => {
  assert(statSync(file).isFile(), `PDF fixture is not a regular file: ${file}`);
};

export const assertPdfSuccess = (response: JsonRpcResponse): void => {
  assert.equal(response.error, undefined, JSON.stringify(response.error));
  assert(response.result, 'missing tool result');
  assert.notEqual(response.result.isError, true, JSON.stringify(response.result));
  const text = (response.result.content ?? []).map((part) => part.text ?? '').join('\n');
  const payload = response.result.structuredContent ?? JSON.parse(text);
  assert(Array.isArray(payload.results), 'missing PDF source results');
  assert(payload.results.length > 0, 'empty PDF source results');
  for (const result of payload.results) {
    assert.equal(result.success, true, JSON.stringify(result));
  }
};
