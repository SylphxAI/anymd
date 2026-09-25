import { describe, expect, it } from 'bun:test';
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { resolveCliPath, resolveServerPath } from './utils/cargoBinaries.js';

const repoRoot = path.resolve(import.meta.dirname, '..');
const rustServerBin = resolveServerPath();
const rustCliBin = resolveCliPath();
const stagedRustBin = path.join(repoRoot, 'bin/native/anymd');
const samplePdf = path.join(repoRoot, 'test/fixtures/sample.pdf');

describe('MCP transport boundary (pure-Rust)', () => {
  it('builds the rmcp stdio server binary for the production process path', () => {
    expect(existsSync(rustServerBin)).toBe(true);
    expect(existsSync(stagedRustBin)).toBe(true);
    expect(existsSync(rustCliBin)).toBe(true);
  });

  it('executes read_pdf through the sole-Rust CLI', () => {
    const cliProbe = spawnSync(rustCliBin, [], {
      cwd: repoRoot,
      encoding: 'utf8',
      input: JSON.stringify({
        tool: 'read_pdf',
        input: {
          sources: [{ path: samplePdf }],
          include_metadata: true,
          include_page_count: true,
          include_full_text: false,
        },
      }),
      timeout: 30_000,
    });

    expect(cliProbe.status).toBe(0);

    const cliEnvelope = JSON.parse(cliProbe.stdout) as {
      status?: string;
      tool?: string;
      result?: { content?: Array<{ text?: string }> };
    };
    expect(cliEnvelope.status).toBe('ok');
    expect(cliEnvelope.tool).toBe('read_pdf');
    const payloadText = cliEnvelope.result?.content?.[0]?.text ?? '';
    expect(payloadText).toContain('rust-read-pdf-v1');
    expect(payloadText).toContain('"success":true');
    expect(payloadText).not.toContain('legacy-engine-runtime');
  });

  it('delegates pdf_hash through pdf-reader-cli JSON boundary', () => {
    const cliProbe = spawnSync(rustCliBin, [], {
      cwd: repoRoot,
      encoding: 'utf8',
      input: JSON.stringify({
        tool: 'pdf_hash',
        input: { path: samplePdf },
      }),
    });
    expect(cliProbe.status).toBe(0);
    const cliEnvelope = JSON.parse(cliProbe.stdout) as {
      status?: string;
      hash?: { sourceHash?: string };
    };
    expect(cliEnvelope.status).toBe('ok');
    expect(cliEnvelope.hash?.sourceHash?.length).toBe(64);
  });

  it('reports doctor diagnostics from the default Rust MCP entrypoint', () => {
    const result = spawnSync(rustServerBin, ['doctor'], {
      cwd: repoRoot,
      encoding: 'utf8',
    });

    const output = `${result.stdout ?? ''}${result.stderr ?? ''}`;
    expect(output).toContain('(native Rust)');
    expect(output).toContain('tesseract');
  });

  it('converts a file to Markdown on stdout in CLI mode', () => {
    const result = spawnSync(rustServerBin, [path.join(repoRoot, 'test/fixtures/sample.pdf')], {
      cwd: repoRoot,
      encoding: 'utf8',
    });
    expect(result.status).toBe(0);
    expect(result.stdout).toContain('<!-- page 1 -->');
  });
});
