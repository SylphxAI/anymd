import { describe, expect, it } from 'bun:test';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

const repoRoot = path.resolve(import.meta.dirname, '..');

/**
 * Sole-Rust production package truth (ADR-0006):
 * - Default entry: pure-Rust native via dist/runtime-entry.js only
 * - No ./typescript export and no bundled TS PDF runtime
 * - Historical TS 3.0.14 remains external LKG only
 */
describe('published sole-Rust production package (no TS production runtime)', () => {
  it('npm package bin is runtime-entry without typescript export', () => {
    const pkg = JSON.parse(readFileSync(path.join(repoRoot, 'package.json'), 'utf8')) as {
      bin?: Record<string, string>;
      exports?: Record<string, string>;
      version?: string;
      files?: string[];
    };
    expect(pkg.bin?.citra).toBe('./dist/runtime-entry.js');
    expect(pkg.exports?.['.']).toBe('./dist/runtime-entry.js');
    expect(pkg.exports?.['./typescript']).toBeUndefined();
    expect(pkg.exports?.['./pure-rust']).toBe('./dist/pure-rust.js');
    expect(pkg.files).toContain('dist/runtime-entry.js');
    expect(pkg.files).toContain('dist/pure-rust.js');
    expect(pkg.files).not.toContain('dist/');
    expect(pkg.version).toMatch(/^\d+\.\d+\.\d+$/);
  });

  it('parity bridge is deleted', () => {
    expect(
      existsSync(path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/parity_bridge.rs'))
    ).toBe(false);
  });

  it('honest capability matrix documents sole-Rust candidate without TS production ship', () => {
    const matrix = JSON.parse(
      readFileSync(path.join(repoRoot, 'docs/specs/pure-rust-capability-matrix.json'), 'utf8')
    ) as {
      productTruth: {
        soleRustProduction?: boolean;
        typescriptProductionShipped?: boolean;
        automaticTypescriptFallback?: boolean;
        pureRustStatus: string;
        publishFreeze?: boolean;
        dropInFor3014?: boolean;
        candidateVersion?: string;
      };
    };
    expect(matrix.productTruth.soleRustProduction).toBe(true);
    expect(matrix.productTruth.typescriptProductionShipped).toBe(false);
    expect(matrix.productTruth.automaticTypescriptFallback).toBe(false);
    expect(matrix.productTruth.publishFreeze).toBe(false);
    expect(matrix.productTruth.dropInFor3014).toBe(true);
    expect(matrix.productTruth.pureRustStatus).toContain('sole-rust');
    expect(matrix.productTruth.candidateVersion).toMatch(/^5\./);
  });
});
