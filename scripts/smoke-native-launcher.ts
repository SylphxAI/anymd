#!/usr/bin/env bun
/**
 * Host pure-Rust native binary smoke (not registry publish proof).
 *
 * Spawns the staged platform binary directly (works on Windows without bash).
 * Verifies MCP initialize. Keeps publishFreeze/dropInFor3014 unchanged.
 */
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import {
  NATIVE_PLATFORM_PACKAGES,
  nativeBinaryRelativePath,
  resolveNativePlatformId,
} from './native/platform-package-map.ts';

const repoRoot = join(import.meta.dirname, '..');
const platformId = resolveNativePlatformId();
if (!platformId) {
  console.error(`unsupported host platform ${process.platform}/${process.arch}`);
  process.exit(2);
}

const meta = NATIVE_PLATFORM_PACKAGES[platformId];
const staged = join(repoRoot, nativeBinaryRelativePath(platformId));
const packageBin = join(repoRoot, meta.packageDir, 'bin', meta.binaryName);
const legacy = join(repoRoot, 'bin/native', meta.binaryName);
const binaryPath = [staged, packageBin, legacy].find((path) => existsSync(path));

if (!binaryPath) {
  console.error(
    `[smoke-native-launcher] missing staged binary for ${platformId}. Run: bun run build:rust && bun scripts/stage-rust-mcp.ts`
  );
  process.exit(1);
}

const child = spawn(binaryPath, [], {
  cwd: repoRoot,
  stdio: ['pipe', 'pipe', 'pipe'],
  env: {
    ...process.env,
    PDF_READER_ENGINE_MODE: 'pure-rust',
    MCP_TRANSPORT: 'stdio',
  },
});

let buffer = '';
let resolved = false;
const fail = (message: string) => {
  if (resolved) return;
  resolved = true;
  child.kill('SIGTERM');
  console.error(`[smoke-native-launcher] ${message}`);
  process.exit(1);
};

const timer = setTimeout(() => fail('timed out waiting for initialize'), 20_000);
child.stderr.on('data', () => {});
child.stdout.on('data', (chunk: Buffer) => {
  buffer += chunk.toString();
  while (buffer.includes('\n')) {
    const index = buffer.indexOf('\n');
    const line = buffer.slice(0, index).trim();
    buffer = buffer.slice(index + 1);
    if (!line) continue;
    let response: Record<string, unknown>;
    try {
      response = JSON.parse(line) as Record<string, unknown>;
    } catch {
      continue;
    }
    if (response.id !== 1) continue;
    const result = response.result as Record<string, unknown> | undefined;
    const serverInfo = result?.serverInfo as Record<string, unknown> | undefined;
    if (serverInfo?.name !== 'anymd') {
      fail(`unexpected serverInfo: ${JSON.stringify(serverInfo)}`);
    }
    clearTimeout(timer);
    resolved = true;
    child.kill('SIGTERM');
    console.log(
      JSON.stringify(
        {
          profile: 'pdf_native_launcher_smoke',
          platformId,
          binaryPath,
          resolutionOrder: [staged, packageBin, legacy],
          serverName: serverInfo?.name,
          serverVersion: serverInfo?.version,
          productTruth: { dropInFor3014: false, publishFreeze: false },
          pass: true,
        },
        null,
        2
      )
    );
    process.exit(0);
  }
});
child.on('exit', (code) => {
  if (!resolved) fail(`binary exited early with code ${String(code)}`);
});

child.stdin.write(
  `${JSON.stringify({
    jsonrpc: '2.0',
    id: 1,
    method: 'initialize',
    params: {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'native-launcher-smoke', version: '1' },
    },
  })}\n`
);
