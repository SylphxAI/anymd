import { describe, expect, it } from 'bun:test';
import { readFileSync } from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');

describe('production dependency closure (sole-Rust)', () => {
  it('declares no production npm dependencies and no banned install-graph packages', () => {
    const pkg = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8')) as {
      dependencies?: Record<string, string>;
      optionalDependencies?: Record<string, string>;
      scripts?: Record<string, string>;
    };
    expect(pkg.dependencies ?? {}).toEqual({});
    for (const banned of ['pdfjs-dist', '@modelcontextprotocol/sdk', 'pngjs', 'zod']) {
      expect(pkg.dependencies?.[banned]).toBeUndefined();
    }
    for (const name of Object.keys(pkg.optionalDependencies ?? {})) {
      expect(name.startsWith('@sylphx/anymd-')).toBe(true);
    }
    expect(pkg.scripts?.build ?? '').not.toContain('oracle-ts');
    expect(pkg.scripts?.['build:oracle-ts']).toBeTruthy();
    expect(pkg.scripts?.prepublishOnly ?? '').not.toContain('build:oracle-ts');
  });

  it('matrix no longer advertises production typescript fallback exports', () => {
    const matrix = JSON.parse(
      readFileSync(path.join(root, 'docs/specs/pure-rust-capability-matrix.json'), 'utf8')
    ) as {
      productTruth: {
        typescriptFallbackExports?: string[];
        typescriptProductionShipped?: boolean;
        typescriptFallback?: boolean | string;
      };
    };
    expect(matrix.productTruth.typescriptProductionShipped).toBe(false);
    expect(matrix.productTruth.typescriptFallback).toBe(false);
    expect(matrix.productTruth.typescriptFallbackExports ?? []).toEqual([]);
  });
});
