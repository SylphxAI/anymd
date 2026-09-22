#!/usr/bin/env bun
/**
 * Sync optional native package manifests to the root package version and Stage B posture.
 * Does not build binaries. Refuse to invent empty publishable packages.
 */
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { NATIVE_PLATFORM_PACKAGES } from '../../src/native/platform-package-map.ts';

const root = join(import.meta.dirname, '../..');
const rootPkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
  version: string;
  optionalDependencies?: Record<string, string>;
};

const version = rootPkg.version;
if (!version) {
  console.error('[native:sync-manifests] root package.json missing version');
  process.exit(1);
}

const prepublishOnly =
  "node -e \"const fs=require('fs');const p=require('./package.json');const bin=p.citraNativeBinary;if(!bin||!fs.existsSync(bin)||fs.statSync(bin).size<1024){console.error('REFUSE PUBLISH: native binary missing or empty at',bin);process.exit(1)}console.log('native binary present:',bin,'bytes',fs.statSync(bin).size);\"";

for (const [platformId, meta] of Object.entries(NATIVE_PLATFORM_PACKAGES)) {
  const pkgPath = join(root, meta.packageDir, 'package.json');
  if (!existsSync(pkgPath)) {
    console.error(`[native:sync-manifests] missing ${pkgPath}`);
    process.exit(1);
  }
  const pkg = JSON.parse(readFileSync(pkgPath, 'utf8')) as Record<string, unknown>;
  pkg.name = meta.npmName;
  pkg.version = version;
  pkg.description =
    `Optional pure-Rust MCP server binary for ${platformId}. ` +
    'Installed as an optionalDependency of @sylphx/citra. Sole-Rust; fail-closed if missing.';
  pkg.license = 'MIT';
  pkg.os = [meta.os];
  pkg.cpu = [meta.cpu];
  pkg.files = ['bin/', 'README.md', 'package.json'];
  pkg.publishConfig = { access: 'public' };
  pkg.citraNativeBinary = `bin/${meta.binaryName}`;
  pkg.scripts = { ...(typeof pkg.scripts === 'object' && pkg.scripts ? pkg.scripts : {}), prepublishOnly };
  pkg.repository = {
    type: 'git',
    url: 'git+https://github.com/SylphxAI/citra.git',
    directory: meta.packageDir,
  };
  pkg.homepage = 'https://github.com/SylphxAI/citra#readme';
  pkg.engines = { node: '>=18' };
  // Stage B: publishable when binary present. Never leave private:true freeze.
  delete pkg.private;
  if (platformId.startsWith('linux-')) {
    pkg.libc = ['glibc'];
  } else {
    delete pkg.libc;
  }
  writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
  console.log(`[native:sync-manifests] ${meta.npmName}@${version}`);
}

// Keep root optionalDependencies aligned.
const optional: Record<string, string> = {};
for (const meta of Object.values(NATIVE_PLATFORM_PACKAGES)) {
  optional[meta.npmName] = version;
}
const rootPkgPath = join(root, 'package.json');
const rootRaw = JSON.parse(readFileSync(rootPkgPath, 'utf8')) as Record<string, unknown>;
rootRaw.optionalDependencies = optional;
writeFileSync(rootPkgPath, `${JSON.stringify(rootRaw, null, 2)}\n`);
console.log(`[native:sync-manifests] root optionalDependencies -> ${version}`);

// Sanity: package dirs match map.
const dirs = readdirSync(join(root, 'packages')).filter((name) => name.startsWith('citra-'));
const expected = Object.keys(NATIVE_PLATFORM_PACKAGES).map((id) => `citra-${id}`);
if (dirs.sort().join() !== expected.sort().join()) {
  console.error('[native:sync-manifests] package dirs drift from platform map', { dirs, expected });
  process.exit(1);
}
console.log('[native:sync-manifests] PASS');
