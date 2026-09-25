#!/usr/bin/env bun
/**
 * Assert optional native packages are publish-ready for the requested platforms.
 *
 * Default: assert host platform only (local/dev).
 * --all: assert every mapped platform has a real binary staged under packages/.../bin
 * --manifests-only: only check package.json posture (no binary size check)
 */
import { existsSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import {
  NATIVE_PLATFORM_PACKAGES,
  type NativePlatformId,
  resolveNativePlatformId,
} from '../../src/native/platform-package-map.ts';

const root = join(import.meta.dirname, '../..');
const all = process.argv.includes('--all');
const manifestsOnly = process.argv.includes('--manifests-only');
const platformArg = process.argv.find((arg) => arg.startsWith('--platform='))?.slice('--platform='.length) as
  | NativePlatformId
  | undefined;
const rootPkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
  version: string;
  optionalDependencies?: Record<string, string>;
};

const failures: string[] = [];
const platformIds = (all
  ? (Object.keys(NATIVE_PLATFORM_PACKAGES) as NativePlatformId[])
  : platformArg
    ? [platformArg]
    : ([resolveNativePlatformId()].filter(Boolean) as NativePlatformId[]));

if (platformArg && !NATIVE_PLATFORM_PACKAGES[platformArg]) {
  failures.push(`unknown --platform=${platformArg}`);
}

if (!platformIds.length) {
  failures.push(`unsupported host platform ${process.platform}/${process.arch}`);
}

for (const platformId of platformIds) {
  const meta = NATIVE_PLATFORM_PACKAGES[platformId];
  const pkgPath = join(root, meta.packageDir, 'package.json');
  if (!existsSync(pkgPath)) {
    failures.push(`missing ${pkgPath}`);
    continue;
  }
  const pkg = JSON.parse(readFileSync(pkgPath, 'utf8')) as {
    name?: string;
    version?: string;
    private?: boolean;
    anymdNativeBinary?: string;
    scripts?: { prepublishOnly?: string };
  };
  if (pkg.name !== meta.npmName) failures.push(`${platformId}: name mismatch ${pkg.name}`);
  if (pkg.version !== rootPkg.version) {
    failures.push(`${platformId}: version ${pkg.version} != root ${rootPkg.version}`);
  }
  if (pkg.private === true) failures.push(`${platformId}: still private; Stage B requires publishable package`);
  if (!pkg.anymdNativeBinary) failures.push(`${platformId}: missing anymdNativeBinary`);
  if (!pkg.scripts?.prepublishOnly?.includes('REFUSE PUBLISH')) {
    failures.push(`${platformId}: prepublishOnly must refuse publish without binary`);
  }
  if (rootPkg.optionalDependencies?.[meta.npmName] !== rootPkg.version) {
    failures.push(`${platformId}: root optionalDependencies missing ${meta.npmName}@${rootPkg.version}`);
  }
  if (!manifestsOnly) {
    const binPath = join(root, meta.packageDir, pkg.anymdNativeBinary ?? `bin/${meta.binaryName}`);
    if (!existsSync(binPath)) {
      failures.push(`${platformId}: binary missing at ${binPath}`);
    } else {
      const size = statSync(binPath).size;
      if (size < 1024) failures.push(`${platformId}: binary too small (${size} bytes) at ${binPath}`);
    }
  }
}

if (failures.length) {
  console.error(failures.map((line) => `[native:assert-ready] ${line}`).join('\n'));
  process.exit(1);
}

console.log(
  JSON.stringify(
    {
      profile: 'native_packages_ready',
      pass: true,
      mode: manifestsOnly
        ? 'manifests-only'
        : all
          ? 'all-platforms-with-binaries'
          : platformArg
            ? 'explicit-platform-with-binary'
            : 'host-with-binary',
      version: rootPkg.version,
      platforms: platformIds,
    },
    null,
    2
  )
);
console.log('[native:assert-ready] PASS');
