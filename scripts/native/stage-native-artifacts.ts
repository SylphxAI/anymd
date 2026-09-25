#!/usr/bin/env bun
/**
 * Stage downloaded CI native binary artifacts into packages/<platform>/bin and bin/native/<platform>/.
 *
 * Expected artifact layout (from native-package-scaffold / publish workflow):
 *   <artifactRoot>/anymd-<platformId>/<binary>
 * or:
 *   <artifactRoot>/<platformId>/<binary>
 * or direct file path via --from=<file> --platform=<id>
 */
import { copyFileSync, existsSync, mkdirSync, readdirSync, statSync, chmodSync } from 'node:fs';
import { basename, join } from 'node:path';
import {
  NATIVE_PLATFORM_PACKAGES,
  type NativePlatformId,
  nativeBinaryRelativePath,
} from '../../src/native/platform-package-map.ts';

const root = join(import.meta.dirname, '../..');
const args = process.argv.slice(2);
const getArg = (name: string): string | null => {
  const hit = args.find((a) => a.startsWith(`${name}=`));
  return hit ? hit.slice(name.length + 1) : null;
};

const artifactRoot = getArg('--artifact-root') ?? join(root, 'native-artifacts');
const onlyPlatform = getArg('--platform') as NativePlatformId | null;
const fromFile = getArg('--from');

const stageOne = (platformId: NativePlatformId, source: string) => {
  const meta = NATIVE_PLATFORM_PACKAGES[platformId];
  if (!existsSync(source) || statSync(source).size < 1024) {
    throw new Error(`source binary missing/empty for ${platformId}: ${source}`);
  }
  const packageBin = join(root, meta.packageDir, 'bin', meta.binaryName);
  const stagedPath = join(root, nativeBinaryRelativePath(platformId));
  for (const dest of [packageBin, stagedPath]) {
    mkdirSync(join(dest, '..'), { recursive: true });
    copyFileSync(source, dest);
    if (platformId !== 'win32-x64-msvc') {
      try {
        chmodSync(dest, 0o755);
      } catch {
        // windows runners may not care
      }
    }
    console.log(`[native:stage-from-artifacts] ${platformId}: ${source} -> ${dest}`);
  }
};

if (fromFile && onlyPlatform) {
  stageOne(onlyPlatform, fromFile);
  console.log('[native:stage-from-artifacts] PASS');
  process.exit(0);
}

if (!existsSync(artifactRoot)) {
  console.error(`[native:stage-from-artifacts] artifact root missing: ${artifactRoot}`);
  process.exit(1);
}

const platforms = (onlyPlatform
  ? [onlyPlatform]
  : (Object.keys(NATIVE_PLATFORM_PACKAGES) as NativePlatformId[]));

for (const platformId of platforms) {
  const meta = NATIVE_PLATFORM_PACKAGES[platformId];
  const candidates = [
    join(artifactRoot, `anymd-native-${platformId}`, meta.binaryName),
    join(artifactRoot, platformId, meta.binaryName),
    join(artifactRoot, `anymd-native-${platformId}`, nativeBinaryRelativePath(platformId)),
    join(artifactRoot, platformId, basename(nativeBinaryRelativePath(platformId))),
  ];
  // Also search one level deep for uploaded directory trees.
  const platformArtifactDir = join(artifactRoot, `anymd-native-${platformId}`);
  if (existsSync(platformArtifactDir)) {
    const walk = (dir: string, depth = 0) => {
      if (depth > 3) return;
      for (const name of readdirSync(dir)) {
        const p = join(dir, name);
        try {
          const st = statSync(p);
          if (st.isDirectory()) walk(p, depth + 1);
          else if (name === meta.binaryName) candidates.unshift(p);
        } catch {
          // ignore
        }
      }
    };
    walk(platformArtifactDir);
  }
  const source = candidates.find((p) => existsSync(p) && statSync(p).size >= 1024);
  if (!source) {
    console.error(`[native:stage-from-artifacts] no binary for ${platformId} under ${artifactRoot}`);
    console.error(`candidates tried:\n${candidates.join('\n')}`);
    process.exit(1);
  }
  stageOne(platformId, source);
}

console.log('[native:stage-from-artifacts] PASS');
