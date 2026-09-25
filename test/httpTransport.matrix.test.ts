import { describe, expect, it } from 'bun:test';
import { readFileSync } from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..');

describe('MCP HTTP transport routing', () => {
  it('sole-Rust runtime entry forwards MCP_TRANSPORT and native server owns HTTP', () => {
    const runtime = readFileSync(path.join(repoRoot, 'src/runtime-entry.ts'), 'utf8');
    const bin = readFileSync(path.join(repoRoot, 'bin/anymd'), 'utf8');
    const httpTransport = readFileSync(
      path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/http_transport.rs'),
      'utf8'
    );

    expect(runtime).toContain('MCP_TRANSPORT');
    expect(bin).toContain('dist/runtime-entry.js');
    expect(bin).not.toContain('dist/index.js');
    expect(httpTransport).toContain('StreamableHttpService');
    expect(httpTransport).toContain('/mcp/health');
  });
});
