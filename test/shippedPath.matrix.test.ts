import { beforeAll, describe, expect, it } from 'bun:test';
import { execSync, spawnSync } from 'node:child_process';
import path from 'node:path';
import { resolveCliPath, resolveServerPath } from './utils/cargoBinaries.js';

const repoRoot = path.resolve(import.meta.dirname, '..');
const rustCliBin = resolveCliPath();
const samplePdf = path.join(repoRoot, 'test/fixtures/sample.pdf');

type CliEnvelope = {
  status?: string;
  code?: string;
  message?: string;
  tool?: string;
  hash?: { sourceHash?: string };
  search?: { route?: string };
  result?: { content?: Array<{ text?: string }> };
};

const invokeCli = (tool: string, input: Record<string, unknown>) => {
  const probe = spawnSync(rustCliBin, [], {
    cwd: repoRoot,
    encoding: 'utf8',
    input: JSON.stringify({ tool, input }),
    timeout: 30_000,
  });
  expect(probe.status).toBe(0);
  return JSON.parse(probe.stdout) as CliEnvelope;
};

describe('shipped path matrix (Rust core)', () => {
  beforeAll(() => {
    execSync('bun run build:rust', { cwd: repoRoot, stdio: 'pipe', timeout: 300_000 });
  }, 300_000);

  it('pdf_hash returns deterministic Rust provenance', () => {
    const envelope = invokeCli('pdf_hash', { path: samplePdf });
    expect(envelope.status).toBe('ok');
    expect(envelope.hash?.sourceHash?.length).toBe(64);
  });

  it('pdf_text_search returns rust-text-index route', () => {
    const envelope = invokeCli('pdf_text_search', { path: samplePdf, query: 'Lorem' });
    expect(envelope.status).toBe('ok');
    expect(envelope.search?.route).toBe('rust-text-index');
  });

  it('read_pdf returns rust-read-pdf-v1 without legacy runtime', () => {
    const envelope = invokeCli('read_pdf', {
      sources: [{ path: samplePdf }],
      include_metadata: true,
      include_page_count: true,
      include_full_text: false,
    });
    expect(envelope.status).toBe('ok');
    expect(envelope.tool).toBe('read_pdf');
    const payload = envelope.result?.content?.[0]?.text ?? '';
    expect(payload).toContain('rust-read-pdf-v1');
    expect(payload.toLowerCase()).not.toContain('typescript');
  });

  it('search_pdf defaults to rust-text-index without legacy runtime', () => {
    const envelope = invokeCli('search_pdf', {
      sources: [{ path: samplePdf }],
      query: 'Lorem',
    });
    expect(envelope.status).toBe('ok');
    expect(envelope.tool).toBe('search_pdf');
    const payload = envelope.result?.content?.[0]?.text ?? '';
    expect(payload).toContain('rust-text-index');
    expect(payload).toContain('"success":true');
    expect(payload.toLowerCase()).not.toContain('typescript');
  });

  it('pdf_evidence inspect returns rust-pdf-inspect-v1 without legacy runtime', () => {
    const envelope = invokeCli('pdf_evidence', {
      operation: 'inspect',
      sources: [{ path: samplePdf }],
    });
    expect(envelope.status).toBe('ok');
    expect(envelope.tool).toBe('pdf_evidence');
    const payload = envelope.result?.content?.[0]?.text ?? '';
    expect(payload).toContain('rust-pdf-inspect-v1');
    expect(payload.toLowerCase()).not.toContain('typescript');
  });
});
