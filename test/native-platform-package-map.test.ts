import { describe, expect, test } from 'bun:test';
import {
  NATIVE_PLATFORM_PACKAGES,
  nativeBinaryRelativePath,
  resolveNativePlatformId,
} from '../scripts/native/platform-package-map.ts';

describe('native platform package map', () => {
  test('maps the five release platforms exactly', () => {
    expect(Object.keys(NATIVE_PLATFORM_PACKAGES).sort()).toEqual([
      'darwin-arm64',
      'darwin-x64',
      'linux-arm64-gnu',
      'linux-x64-gnu',
      'win32-x64-msvc',
    ]);
  });

  test('resolves host triples used by the optional package design', () => {
    expect(resolveNativePlatformId('darwin', 'arm64')).toBe('darwin-arm64');
    expect(resolveNativePlatformId('darwin', 'x64')).toBe('darwin-x64');
    expect(resolveNativePlatformId('linux', 'arm64')).toBe('linux-arm64-gnu');
    expect(resolveNativePlatformId('linux', 'x64')).toBe('linux-x64-gnu');
    expect(resolveNativePlatformId('win32', 'x64')).toBe('win32-x64-msvc');
    expect(resolveNativePlatformId('freebsd', 'x64')).toBeNull();
  });

  test('uses platform-scoped staged binary paths', () => {
    expect(nativeBinaryRelativePath('linux-x64-gnu')).toBe('bin/native/linux-x64-gnu/anymd');
    expect(nativeBinaryRelativePath('win32-x64-msvc')).toBe('bin/native/win32-x64-msvc/anymd.exe');
  });

  test('package metadata is publishable and binary-gated (Stage B)', async () => {
    const { readdirSync, readFileSync } = await import('node:fs');
    const { join } = await import('node:path');
    const root = join(import.meta.dir, '..');
    const rootPkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
      version: string;
      optionalDependencies?: Record<string, string>;
    };
    for (const platformId of Object.keys(NATIVE_PLATFORM_PACKAGES)) {
      const meta = NATIVE_PLATFORM_PACKAGES[platformId as keyof typeof NATIVE_PLATFORM_PACKAGES];
      const pkg = JSON.parse(
        readFileSync(join(root, `packages/anymd-${platformId}/package.json`), 'utf8')
      ) as {
        private?: boolean;
        name?: string;
        version?: string;
        anymdNativeBinary?: string;
        scripts?: { prepublishOnly?: string };
      };
      expect(pkg.private).not.toBe(true);
      expect(pkg.name).toBe(meta.npmName);
      expect(pkg.version).toBe(rootPkg.version);
      expect(pkg.anymdNativeBinary).toBe(`bin/${meta.binaryName}`);
      expect(pkg.scripts?.prepublishOnly ?? '').toContain('REFUSE PUBLISH');
      expect(rootPkg.optionalDependencies?.[meta.npmName]).toBe(rootPkg.version);
    }
    const dirs = readdirSync(join(root, 'packages')).filter((name) => name.startsWith('anymd-'));
    expect(dirs.sort()).toEqual(
      Object.keys(NATIVE_PLATFORM_PACKAGES)
        .map((id) => `anymd-${id}`)
        .sort()
    );
  });
});
