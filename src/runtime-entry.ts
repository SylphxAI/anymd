#!/usr/bin/env node
/**
 * Sole-Rust production package entry for @sylphx/anymd.
 *
 * Launches the platform optional pure-Rust MCP server binary only.
 * There is no TypeScript PDF processing fallback in the production package.
 */
import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  NATIVE_PLATFORM_PACKAGES,
  resolveNativePlatformId,
} from './native/platform-package-map.js';

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));
const packageRoot = join(here, '..');
const packageVersion = String(
  (JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8')) as { version?: string })
    .version ?? ''
);

const versionedPackageBinary = (nativePackageRoot: string, binaryName: string): string | null => {
  const binary = join(nativePackageRoot, 'bin', binaryName);
  if (!existsSync(binary)) return null;
  try {
    const nativeVersion = String(
      (
        JSON.parse(readFileSync(join(nativePackageRoot, 'package.json'), 'utf8')) as {
          version?: string;
        }
      ).version ?? ''
    );
    if (nativeVersion !== packageVersion) {
      console.error(
        `[anymd] refusing native package version ${nativeVersion || 'unknown'}; wrapper version is ${packageVersion || 'unknown'}.`
      );
      return null;
    }
  } catch {
    return null;
  }
  return binary;
};

const resolveNativeBinary = (): string | null => {
  const platformId = resolveNativePlatformId();
  if (!platformId) return null;
  const meta = NATIVE_PLATFORM_PACKAGES[platformId];
  const candidates: string[] = [];
  const workspacePackage = versionedPackageBinary(
    join(packageRoot, meta.packageDir),
    meta.binaryName
  );
  if (workspacePackage) candidates.push(workspacePackage);

  // Source-checkout staging path. This directory is excluded from the npm package.
  candidates.push(join(packageRoot, 'bin/native', platformId, meta.binaryName));

  const installedPackage = versionedPackageBinary(
    join(packageRoot, 'node_modules', meta.npmName),
    meta.binaryName
  );
  if (installedPackage) candidates.push(installedPackage);
  try {
    const pkgJson = require.resolve(`${meta.npmName}/package.json`, {
      paths: [packageRoot],
    });
    const resolvedPackage = versionedPackageBinary(dirname(pkgJson), meta.binaryName);
    if (resolvedPackage) candidates.push(resolvedPackage);
  } catch {
    // optional dependency not installed
  }
  for (const candidate of candidates) {
    if (existsSync(candidate)) return candidate;
  }
  return null;
};

if (
  process.env['PDF_READER_FORCE_TYPESCRIPT'] === '1' ||
  process.env['PDF_READER_ENGINE_MODE'] === 'typescript' ||
  process.env['PDF_READER_ENGINE_MODE'] === 'ts'
) {
  console.error(
    [
      '[anymd] TypeScript production runtime has been removed from this package.',
      'Use the immutable historical LKG @sylphx/pdf-reader-mcp@3.0.14 for TypeScript rollback,',
      'or install/run the pure-Rust native binary for this package version.',
    ].join('\n')
  );
  process.exit(2);
}

const nativeBinary = resolveNativeBinary();
if (!nativeBinary) {
  const platformId = resolveNativePlatformId();
  const platformLabel = platformId ?? `${process.platform}/${process.arch}`;
  console.error(
    [
      `[anymd] pure-Rust native binary not found for ${platformLabel}.`,
      'anymd is sole-Rust: there is no bundled TypeScript PDF runtime.',
      'Install the matching optional native package at the same anymd version.',
      'Historical TypeScript LKG remains available only as @sylphx/pdf-reader-mcp@3.0.14 (external pin).',
    ].join('\n')
  );
  process.exit(1);
}

const child = spawn(nativeBinary, process.argv.slice(2), {
  stdio: 'inherit',
  env: {
    ...process.env,
    PDF_READER_ENGINE_MODE: process.env['PDF_READER_ENGINE_MODE'] || 'pure-rust',
    MCP_TRANSPORT: process.env['MCP_TRANSPORT'] || 'stdio',
  },
});
child.once('error', (error) => {
  console.error(`[anymd] failed to start native server: ${error.message}`);
  process.exit(1);
});

for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP'] as const) {
  process.once(signal, () => {
    if (!child.killed) child.kill(signal);
  });
}

child.on('exit', (code, signal) => {
  if (signal) {
    process.exit(128 + (signal === 'SIGINT' ? 2 : signal === 'SIGTERM' ? 15 : 1));
    return;
  }
  process.exit(code ?? 1);
});
