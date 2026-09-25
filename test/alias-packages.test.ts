import { describe, expect, test } from 'bun:test';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ALIAS_PACKAGES, aliasManifest, MAIN_PACKAGE } from '../scripts/sync-alias-packages.ts';

const root = join(import.meta.dir, '..');
const rootPkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
  name: string;
  version: string;
};

describe('compatibility alias packages', () => {
  test('cover both former npm names', () => {
    expect(ALIAS_PACKAGES.map((alias) => alias.npmName).sort()).toEqual([
      '@sylphx/citra',
      '@sylphx/pdf-reader-mcp',
    ]);
    expect(rootPkg.name).toBe(MAIN_PACKAGE);
  });

  for (const alias of ALIAS_PACKAGES) {
    test(`${alias.npmName} is in lockstep with ${MAIN_PACKAGE}`, () => {
      const manifest = JSON.parse(
        readFileSync(join(root, alias.packageDir, 'package.json'), 'utf8')
      ) as Record<string, unknown>;
      expect(manifest).toEqual(aliasManifest(alias, rootPkg.version));
      expect(manifest['version']).toBe(rootPkg.version);
      expect(manifest['dependencies']).toEqual({ [MAIN_PACKAGE]: rootPkg.version });
      expect(manifest['mcpName']).toBeUndefined();
    });

    test(`${alias.npmName} bin runs the anymd launcher`, () => {
      const binPath = join(root, alias.packageDir, 'bin', `${alias.bin}.js`);
      expect(existsSync(binPath)).toBe(true);
      const source = readFileSync(binPath, 'utf8');
      expect(source.startsWith('#!/usr/bin/env node\n')).toBe(true);
      expect(source).toContain(`import '${MAIN_PACKAGE}';`);
      expect(existsSync(join(root, alias.packageDir, 'README.md'))).toBe(true);
    });
  }
});
