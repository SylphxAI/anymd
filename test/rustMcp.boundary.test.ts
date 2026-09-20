import { beforeAll, describe, expect, it } from 'bun:test';
import { execSync, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { resolveCliPath, resolveServerPath } from './utils/cargoBinaries.js';

const repoRoot = path.resolve(import.meta.dirname, '..');
const rustServerBin = resolveServerPath();
const rustCliBin = resolveCliPath();
const stagedRustBin = path.join(repoRoot, 'bin/native/citra-mcp-server');
const samplePdf = path.join(repoRoot, 'test/fixtures/sample.pdf');

describe('MCP transport boundary (pure-Rust)', () => {
  beforeAll(() => {
    execSync('bun run build:rust', { cwd: repoRoot, stdio: 'pipe', timeout: 300_000 });
  }, 300_000);

  it('builds the rmcp stdio server binary for the production process path', () => {
    expect(existsSync(rustServerBin)).toBe(true);
    expect(existsSync(stagedRustBin)).toBe(true);
    expect(existsSync(rustCliBin)).toBe(true);
  });

  it('does not ship a parity bridge', () => {
    expect(
      existsSync(path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/parity_bridge.rs'))
    ).toBe(false);
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
    expect(output).toContain('Rust MCP server');
    expect(output).toContain('sole-Rust citra-mcp-server');
  });
});
