#!/usr/bin/env bun
/**
 * anymd public proof — local PDF read with citeable structure on sample.pdf.
 */
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dir, '..');
const sample = join(root, 'test/fixtures/sample.pdf');
const outDir = process.env.MCP_PDF_BENCHMARK_OUTPUT_DIR
  ? join(root, process.env.MCP_PDF_BENCHMARK_OUTPUT_DIR)
  : join(root, 'benchmark-artifacts');

if (!existsSync(sample)) {
  console.error('missing sample.pdf', sample);
  process.exit(1);
}

const started = performance.now();
let ok = false;
let error: string | undefined;
let pages: number | undefined;
let hasText = false;
try {
  // Prefer built SDK if present
  const mod = await import('../dist/sdk.js').catch(() => import('../src/sdk.ts'));
  const anymd = (mod as { Anymd?: { create: () => { read: (i: Record<string, unknown>) => Promise<unknown> } } }).Anymd
    ?? (mod as { default?: { create: () => { read: (i: Record<string, unknown>) => Promise<unknown> } } }).default;
  if (!anymd?.create) {
    throw new Error('anymd SDK export not found');
  }
  const result = await anymd.create().read({ sources: [{ path: sample, pages: [1] }] }) as {
    isError?: boolean;
    payload?: unknown;
  };
  if (result?.isError) {
    throw new Error(JSON.stringify(result.payload ?? result).slice(0, 400));
  }
  const text = JSON.stringify(result.payload ?? result);
  hasText = text.length > 20;
  const m = text.match(/"pageCount"\s*:\s*(\d+)/);
  if (m && m[1]) pages = Number(m[1]);
  ok = hasText && !result?.isError;
} catch (e) {
  error = e instanceof Error ? e.message : String(e);
  ok = false;
}
const ms = performance.now() - started;
const report = {
  product: 'anymd',
  sample,
  ms,
  ok,
  error,
  hasText,
  pages,
  hasSkill: existsSync(join(root, 'skills/anymd/SKILL.md')),
  brandPublishDoc: existsSync(join(root, 'docs/BRAND_PUBLISH.md')),
  generatedAt: new Date().toISOString(),
};
mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, 'citra_public_proof.json'), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report, null, 2));
process.exit(ok ? 0 : 1);
