import { describe, expect, it } from 'bun:test';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..');

describe('architecture contract (sole-Rust production package)', () => {
  it('published package points at sole-Rust runtime-entry only', () => {
    const pkg = JSON.parse(readFileSync(path.join(repoRoot, 'package.json'), 'utf8')) as {
      bin?: Record<string, string>;
      exports?: Record<string, string>;
      version?: string;
    };
    expect(pkg.bin?.anymd).toBe('./dist/runtime-entry.js');
    expect(pkg.exports?.['.']).toBe('./dist/runtime-entry.js');
    expect(pkg.exports?.['./typescript']).toBeUndefined();
    expect(pkg.version).toMatch(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/);
  });

  it('pure-Rust tool modules exist without parity bridge', () => {
    expect(existsSync(path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/read_pdf.rs'))).toBe(
      true
    );
    expect(existsSync(path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/search.rs'))).toBe(
      true
    );
    expect(
      existsSync(path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/pdf_evidence.rs'))
    ).toBe(true);
    expect(
      existsSync(path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/parity_bridge.rs'))
    ).toBe(false);
  });
});
