#!/usr/bin/env bun
/**
 * Simulated optional native package install/resolve proof for the host platform.
 *
 * This is NOT registry publish proof and does not unfreeze publish.
 * It proves the optional package layout + node_modules resolution path used by
 * bin/anymd and packaging docs can locate and run the pure-Rust binary.
 */
import { spawn } from 'node:child_process';
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { resolveCargoReleaseDir } from './native/cargo-target-dir.ts';
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
const sourceCandidates = [
  join(repoRoot, nativeBinaryRelativePath(platformId)),
  join(repoRoot, meta.packageDir, 'bin', meta.binaryName),
  join(resolveCargoReleaseDir(), meta.binaryName),
  join(repoRoot, 'bin/native', meta.binaryName),
];
const sourceBinary = sourceCandidates.find((path) => existsSync(path));
if (!sourceBinary) {
  console.error(
    `[smoke-native-package-resolve] missing host binary for ${platformId}. Build/stage first.`
  );
  process.exit(1);
}

const packageJson = JSON.parse(
  readFileSync(join(repoRoot, meta.packageDir, 'package.json'), 'utf8')
) as {
  name: string;
  private?: boolean;
  version?: string;
};

const tempRoot = mkdtempSync(join(tmpdir(), 'pdf-reader-native-pkg-'));
const moduleRoot = join(tempRoot, 'node_modules', packageJson.name);
const moduleBinDir = join(moduleRoot, 'bin');
const moduleBinary = join(moduleBinDir, meta.binaryName);

try {
  mkdirSync(moduleBinDir, { recursive: true });
  writeFileSync(
    join(moduleRoot, 'package.json'),
    `${JSON.stringify(
      {
        name: packageJson.name,
        version: packageJson.version ?? '0.0.0-packaging-scaffold',
        private: true,
        os: [meta.os],
        cpu: [meta.cpu],
        description: 'Simulated optional native package install for runtime proof only',
      },
      null,
      2
    )}\n`
  );
  cpSync(sourceBinary, moduleBinary);
  if (!platformId.startsWith('win32')) {
    try {
      chmodSync(moduleBinary, 0o755);
    } catch {
      // ignore
    }
  }

  if (!existsSync(moduleBinary)) {
    throw new Error(`failed to stage simulated package binary at ${moduleBinary}`);
  }

  const child = spawn(moduleBinary, [], {
    cwd: tempRoot,
    stdio: ['pipe', 'pipe', 'pipe'],
    env: {
      ...process.env,
      MCP_TRANSPORT: 'stdio',
      PDF_READER_ENGINE_MODE: 'pure-rust',
    },
  });

  let buffer = '';
  let resolved = false;
  const fail = (message: string) => {
    if (resolved) return;
    resolved = true;
    child.kill('SIGTERM');
    console.error(`[smoke-native-package-resolve] ${message}`);
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
            profile: 'pdf_native_package_resolve_smoke',
            platformId,
            npmName: packageJson.name,
            simulatedInstallRoot: tempRoot,
            moduleBinary,
            sourceBinary,
            serverName: serverInfo?.name,
            serverVersion: serverInfo?.version,
            packagePrivate: packageJson.private === true,
            productTruth: { dropInFor3014: false, publishFreeze: false },
            notes: [
              'Simulated node_modules optional-package resolution only.',
              'Not npm registry publish/install proof.',
              'publishFreeze may be false under verified-candidate admission.',
            ],
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
    if (!resolved) fail(`package binary exited early with code ${String(code)}`);
  });

  child.stdin.write(
    `${JSON.stringify({
      jsonrpc: '2.0',
      id: 1,
      method: 'initialize',
      params: {
        protocolVersion: '2024-11-05',
        capabilities: {},
        clientInfo: { name: 'native-package-resolve-smoke', version: '1' },
      },
    })}\n`
  );
} finally {
  // cleanup is best-effort after process exit; keep temp for debugging on failure.
  process.on('exit', () => {
    try {
      rmSync(tempRoot, { recursive: true, force: true });
    } catch {
      // ignore
    }
  });
}
