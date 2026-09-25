import { describe, expect, it } from 'bun:test';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..');

const readText = (relativePath: string): string =>
  readFileSync(path.join(repoRoot, relativePath), 'utf8');

describe('MCP stdio production default (sole-Rust package)', () => {
  it('default npm path is sole-Rust runtime-entry.js', () => {
    const pkg = JSON.parse(readText('package.json')) as {
      bin?: Record<string, string>;
      exports?: Record<string, string>;
    };
    const bin = readText('bin/anymd');

    expect(pkg.bin?.anymd).toBe('./dist/runtime-entry.js');
    expect(pkg.exports?.['.']).toBe('./dist/runtime-entry.js');
    expect(pkg.exports?.['./typescript']).toBeUndefined();
    expect(bin).toContain('dist/runtime-entry.js');
    expect(bin).not.toContain('dist/index.js');
  });

  it('parity bridge is removed from the pure-Rust server sources', () => {
    expect(
      existsSync(path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/parity_bridge.rs'))
    ).toBe(false);
  });

  it('stdio integration harness remains present', () => {
    expect(existsSync(path.join(repoRoot, 'test/stdioTransport.matrix.test.ts'))).toBe(true);
  });
});
