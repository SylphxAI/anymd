import { describe, expect, test } from 'bun:test';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { Anymd, Citra } from '../src/sdk.ts';

const root = join(import.meta.dir, '..');

describe('anymd SDK export', () => {
  test('exports Anymd class and create factory', () => {
    expect(typeof Anymd).toBe('function');
    expect(typeof Anymd.create).toBe('function');
  });

  test('keeps the deprecated Citra name as the same class', () => {
    expect(Citra).toBe(Anymd);
  });

  test('package.json exports brand SDK surface and anymd bin only', () => {
    const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
      name?: string;
      exports?: Record<string, string>;
      bin?: Record<string, string>;
      files?: string[];
    };
    expect(pkg.name).toBe('@sylphx/anymd');
    expect(pkg.exports?.['./sdk']).toBe('./dist/sdk.js');
    expect(pkg.exports?.['./citra']).toBeUndefined();
    expect(pkg.bin?.anymd).toBeTruthy();
    // Old bins live in the thin alias packages, never in the main package.
    expect(pkg.bin?.citra).toBeUndefined();
    expect(pkg.bin?.['pdf-reader-mcp']).toBeUndefined();
    expect(Object.keys(pkg.bin ?? {})).toEqual(['anymd']);
    expect(pkg.files ?? []).toContain('dist/sdk.js');
  });

  test('dist/sdk.js exists after package build', () => {
    const sdkDist = join(root, 'dist/sdk.js');
    // ensure source always available
    expect(existsSync(join(root, 'src/sdk.ts'))).toBe(true);
    if (existsSync(join(root, 'dist/pure-rust.js'))) {
      expect(existsSync(sdkDist)).toBe(true);
    }
  });
});

test('marketplace server.json brands as anymd', () => {
  const server = JSON.parse(readFileSync(join(root, 'server.json'), 'utf8')) as {
    title?: string;
  };
  expect(server.title).toBe('anymd');
});
