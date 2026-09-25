#!/usr/bin/env bun
/**
 * Keep the thin compatibility alias packages in lockstep with @sylphx/anymd.
 *
 * @sylphx/citra and @sylphx/pdf-reader-mcp are former names of @sylphx/anymd.
 * Each alias publishes at the main package's exact version, depends on
 * @sylphx/anymd at that exact version, and exposes its old bin name, so old
 * install commands (`npx -y @sylphx/citra`, `npx -y @sylphx/pdf-reader-mcp`)
 * keep working.
 *
 * Usage:
 *   bun scripts/sync-alias-packages.ts          # rewrite manifests to root version
 *   bun scripts/sync-alias-packages.ts --check  # fail if any manifest drifted
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import product from '../product.json' with { type: 'json' };

export const MAIN_PACKAGE = '@sylphx/anymd';

export const ALIAS_PACKAGES = [
  { npmName: '@sylphx/citra', bin: 'citra', packageDir: 'packages/alias-citra', formerly: 'Citra' },
  {
    npmName: '@sylphx/pdf-reader-mcp',
    bin: 'pdf-reader-mcp',
    packageDir: 'packages/alias-pdf-reader-mcp',
    formerly: 'pdf-reader-mcp',
  },
] as const;

export type AliasPackage = (typeof ALIAS_PACKAGES)[number];

export const aliasManifest = (alias: AliasPackage, version: string): Record<string, unknown> => ({
  name: alias.npmName,
  version,
  description: `${alias.formerly} is now ${MAIN_PACKAGE}. ${product.tagline}. Compatibility alias that runs anymd as \`${alias.bin}\`.`,
  type: 'module',
  bin: { [alias.bin]: `./bin/${alias.bin}.js` },
  files: ['bin/', 'README.md'],
  dependencies: { [MAIN_PACKAGE]: version },
  publishConfig: { access: 'public' },
  engines: { node: '>=18' },
  repository: {
    type: 'git',
    url: 'git+https://github.com/SylphxAI/anymd.git',
    directory: alias.packageDir,
  },
  bugs: { url: 'https://github.com/SylphxAI/anymd/issues' },
  homepage: 'https://sylphxai.github.io/anymd/',
  author: 'Sylphx <contact@sylphx.com> (https://sylphx.com)',
  license: 'MIT',
  keywords: ['anymd', 'markdown', 'mcp', 'pdf-to-markdown', alias.bin],
});

const render = (manifest: Record<string, unknown>): string => `${JSON.stringify(manifest, null, 2)}\n`;

if (import.meta.main) {
  const root = join(import.meta.dirname, '..');
  const check = process.argv.includes('--check');
  const rootPkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
    name?: string;
    version?: string;
  };
  if (rootPkg.name !== MAIN_PACKAGE || !rootPkg.version) {
    console.error(`[sync-alias-packages] root package must be ${MAIN_PACKAGE} with a version`);
    process.exit(1);
  }
  const drift: string[] = [];
  for (const alias of ALIAS_PACKAGES) {
    const pkgPath = join(root, alias.packageDir, 'package.json');
    const binPath = join(root, alias.packageDir, 'bin', `${alias.bin}.js`);
    if (!existsSync(binPath)) drift.push(`${alias.npmName}: missing ${binPath}`);
    const expected = render(aliasManifest(alias, rootPkg.version));
    const actual = existsSync(pkgPath) ? readFileSync(pkgPath, 'utf8') : '';
    if (actual === expected) continue;
    if (check) {
      drift.push(`${alias.npmName}: manifest drifted from ${MAIN_PACKAGE}@${rootPkg.version}`);
      continue;
    }
    writeFileSync(pkgPath, expected);
    console.log(`[sync-alias-packages] ${alias.npmName}@${rootPkg.version}`);
  }
  if (drift.length) {
    console.error(drift.map((line) => `[sync-alias-packages] ${line}`).join('\n'));
    process.exit(1);
  }
  console.log('[sync-alias-packages] PASS');
}
