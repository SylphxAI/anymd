import { describe, expect, it } from 'bun:test';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..');

const readText = (relativePath: string): string =>
  readFileSync(path.join(repoRoot, relativePath), 'utf8');

describe('Web MCP HTTP (sole-Rust production package)', () => {
  it('parity bridge is not present', () => {
    expect(
      existsSync(path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/parity_bridge.rs'))
    ).toBe(false);
  });

  it('sole-Rust launcher and HTTP transport remain available', () => {
    const bin = readText('bin/anymd');
    const runtime = readText('src/runtime-entry.ts');
    const httpTransport = readText('crates/pdf-reader-mcp-server/src/http_transport.rs');

    expect(bin).toContain('dist/runtime-entry.js');
    expect(bin).not.toContain('dist/index.js');
    expect(runtime).toContain('MCP_TRANSPORT');
    expect(httpTransport).toContain('StreamableHttpService');
    expect(httpTransport).toContain('health_check');
  });

  it('pure-Rust tool modules call core without parity bridge', () => {
    const read = readText('crates/pdf-reader-mcp-server/src/read_pdf.rs');
    expect(read).toContain('read_pdf_from_value');
    expect(read).not.toContain('parity_bridge');
  });
});
