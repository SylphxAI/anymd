#!/usr/bin/env bun
/**
 * Five-platform registry install + pure-Rust runtime proof harness.
 *
 * Modes:
 * - plan (default): print required proof steps without mutating registries
 * - local-pack: prove host platform install from a packed tarball + staged native binary
 * - registry: prove npm install of a published version and pure-Rust MCP initialize
 *
 * Sole-runtime cutover remains unauthorized until registry mode is green on all five platforms.
 */
import { type ChildProcessWithoutNullStreams, spawn, spawnSync } from 'node:child_process';
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import {
  NATIVE_PLATFORM_PACKAGES,
  type NativePlatformId,
  resolveNativePlatformId,
} from '../src/native/platform-package-map.ts';

const root = join(import.meta.dirname, '..');
const mode = process.argv.includes('--registry')
  ? 'registry'
  : process.argv.includes('--local-pack')
    ? 'local-pack'
    : 'plan';

const platformId = resolveNativePlatformId();

const hostRuntimeFacts = () => {
  const uname = spawnSync('uname', ['-m'], { encoding: 'utf8' });
  const machine = (uname.status === 0 ? uname.stdout.trim() : '') || process.arch;
  let rosettaTranslated: boolean | null = null;
  if (process.platform === 'darwin') {
    const translated = spawnSync('sysctl', ['-in', 'sysctl.proc_translated'], { encoding: 'utf8' });
    if (translated.status === 0) {
      rosettaTranslated = translated.stdout.trim() === '1';
    }
  }
  return {
    nodePlatform: process.platform,
    nodeArch: process.arch,
    unameMachine: machine,
    rosettaTranslated,
    resolvedPlatformId: platformId,
  };
};

const hostMatchesPlatformId = (id: string | null): boolean => {
  if (!id) return false;
  const facts = hostRuntimeFacts();
  const arch = facts.nodeArch;
  const machine = facts.unameMachine;
  if (id.startsWith('darwin-')) {
    // Rosetta x86_64 userspace on Apple Silicon is not a Darwin x64 host proof.
    if (facts.rosettaTranslated) return id.endsWith('arm64') ? false : false;
    if (id.endsWith('arm64')) return arch === 'arm64' || machine === 'arm64';
    if (id.endsWith('x64')) {
      return (
        !facts.rosettaTranslated &&
        (arch === 'x64' || machine === 'x86_64') &&
        arch !== 'arm64' &&
        machine !== 'arm64'
      );
    }
  }
  if (id.startsWith('linux-')) {
    if (id.includes('arm64')) return arch === 'arm64' || machine === 'aarch64' || machine === 'arm64';
    if (id.includes('x64')) return arch === 'x64' || machine === 'x86_64';
  }
  if (id.startsWith('win32-')) {
    return process.platform === 'win32' && (arch === 'x64' || machine === 'x86_64');
  }
  return false;
};

const versionArg = process.argv.find((arg) => arg.startsWith('--version='))?.slice('--version='.length);
const fromVersionArg = process.argv
  .find((arg) => arg.startsWith('--from-version='))
  ?.slice('--from-version='.length);
const skipInitialize = process.argv.includes('--install-only');
const keepTemp = process.argv.includes('--keep-temp');

const plan = {
  profile: 'registry_install_proof',
  mode,
  hostPlatformId: platformId,
  host: hostRuntimeFacts(),
  hostMatchesResolvedPlatformId: hostMatchesPlatformId(platformId),
  requiredPlatforms: Object.keys(NATIVE_PLATFORM_PACKAGES),
  soleRuntimePrerequisite: [
    'productTruth.dropInFor3014=true',
    'native optional packages published with platform binaries',
    'npm install @sylphx/anymd@<version> resolves optional native package',
    'MCP initialize succeeds through the installed anymd launcher on each platform',
  ],
  commands: {
    plan: 'bun scripts/check-registry-install-proof.ts',
    localPack: 'bun scripts/check-registry-install-proof.ts --local-pack',
    registry:
      'bun scripts/check-registry-install-proof.ts --registry --version=<published>',
    registryUpgrade:
      'bun scripts/check-registry-install-proof.ts --registry --from-version=<previous> --version=<published>',
    registryInstallOnly:
      'bun scripts/check-registry-install-proof.ts --registry --install-only --version=<published>',
  },
};

const fail = (message: string, code = 1): never => {
  console.error(`[registry-install-proof] ${message}`);
  process.exit(code);
};

const resolveNativeBinaryFromInstall = (
  installRoot: string,
  id: NativePlatformId
): string | null => {
  const meta = NATIVE_PLATFORM_PACKAGES[id];
  const candidates = [
    join(installRoot, 'node_modules', meta.npmName, 'bin', meta.binaryName),
    join(
      installRoot,
      'node_modules',
      '@sylphx',
      'anymd',
      'node_modules',
      meta.npmName,
      'bin',
      meta.binaryName
    ),
  ];
  for (const candidate of candidates) {
    if (existsSync(candidate) && statSync(candidate).size >= 1024) {
      try {
        if (id !== 'win32-x64-msvc') chmodSync(candidate, 0o755);
      } catch {
        // best-effort
      }
      return candidate;
    }
  }
  return null;
};

const resolveInstalledLauncher = (
  packageDir: string,
  bin: string
): { command: string; args: string[]; path: string } => {
  const launcherPath = resolve(packageDir, bin);
  const packagePrefix = `${resolve(packageDir)}${process.platform === 'win32' ? '\\' : '/'}`;
  if (!launcherPath.startsWith(packagePrefix) || !existsSync(launcherPath)) {
    fail(`installed anymd launcher is missing or escapes its package: ${bin}`);
  }
  return {
    command: process.execPath,
    args: [launcherPath],
    path: launcherPath,
  };
};

const mcpInitialize = async (
  command: string,
  args: string[],
  cwd: string,
  expectedVersion: string
): Promise<{ serverName: string; serverVersion: string }> => {
  return await new Promise((resolve, reject) => {
    const child: ChildProcessWithoutNullStreams = spawn(command, args, {
      cwd,
      stdio: ['pipe', 'pipe', 'pipe'],
      env: {
        ...process.env,
        MCP_TRANSPORT: 'stdio',
        PDF_READER_ENGINE_MODE: 'pure-rust',
      },
    });

    let buffer = '';
    let err = '';
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      child.kill('SIGTERM');
      reject(
        new Error(
          `timed out waiting for initialize from installed launcher ${command} ${args.join(' ')}; stderr=${err.slice(0, 500)}`
        )
      );
    }, 20_000);

    const finish = (err?: Error, value?: { serverName: string; serverVersion: string }) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        child.kill('SIGTERM');
      } catch {
        // ignore
      }
      if (err) reject(err);
      else resolve(value!);
    };

    child.stderr.on('data', (chunk: Buffer) => {
      err += chunk.toString();
    });
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
        const serverName = String(serverInfo?.name ?? '');
        const serverVersion = String(serverInfo?.version ?? '');
        if (serverName !== 'anymd') {
          finish(new Error(`unexpected serverInfo.name=${serverName}`));
          return;
        }
        if (serverVersion !== expectedVersion) {
          finish(
            new Error(
              `installed launcher returned server version ${serverVersion}; expected ${expectedVersion}`
            )
          );
          return;
        }
        finish(undefined, { serverName, serverVersion });
      }
    });
    child.on('error', (error) => finish(error));
    child.on('exit', (code) => {
      if (!settled) {
        finish(
          new Error(
            `binary exited early with code ${String(code)}; stderr=${err.slice(0, 800)}`
          )
        );
      }
    });

    child.stdin.write(
      `${JSON.stringify({
        jsonrpc: '2.0',
        id: 1,
        method: 'initialize',
        params: {
          protocolVersion: '2024-11-05',
          capabilities: {},
          clientInfo: { name: 'registry-install-proof', version: '1' },
        },
      })}\n`
    );
  });
};

if (mode === 'plan') {
  console.log(JSON.stringify({ ...plan, pass: true }, null, 2));
  process.exit(0);
}

if (!platformId) {
  fail(`unsupported host platform: ${process.platform}/${process.arch}`, 2);
}

const requiredPlatformId = process.env['PROOF_REQUIRE_PLATFORM_ID'] || '';
if (requiredPlatformId) {
  const facts = hostRuntimeFacts();
  if (platformId !== requiredPlatformId || !hostMatchesPlatformId(requiredPlatformId)) {
    console.log(
      JSON.stringify(
        {
          profile: 'registry_install_proof',
          mode,
          pass: false,
          runtimeProofValid: false,
          requiredPlatformId,
          host: facts,
          resolvedPlatformId: platformId,
          note: `Host runtime proof for ${requiredPlatformId} is unavailable on this runner (${facts.nodePlatform}/${facts.nodeArch}, uname=${facts.unameMachine}).`,
        },
        null,
        2
      )
    );
    fail(
      `host does not provide runtime proof for required platform ${requiredPlatformId}; resolved=${platformId} arch=${facts.nodeArch}/${facts.unameMachine}`,
      3
    );
  }
}

if (mode === 'local-pack') {
  const meta = NATIVE_PLATFORM_PACKAGES[platformId];
  const staged = join(root, 'packages', `anymd-${platformId}`, 'bin', meta.binaryName);
  if (!existsSync(staged)) {
    fail(
      `missing staged package binary at ${staged}; run bun run build:rust && bun scripts/stage-rust-mcp.ts`
    );
  }
  const temp = mkdtempSync(join(tmpdir(), 'pdf-reader-registry-proof-'));
  try {
    const packMain = spawnSync('npm', ['pack', '--pack-destination', temp], {
      cwd: root,
      encoding: 'utf8',
    });
    if (packMain.status !== 0) {
      fail(packMain.stderr || packMain.stdout || 'npm pack main failed');
    }
    const packNative = spawnSync('npm', ['pack', '--pack-destination', temp], {
      cwd: join(root, meta.packageDir),
      encoding: 'utf8',
    });
    if (packNative.status !== 0) {
      fail(packNative.stderr || packNative.stdout || 'npm pack native failed');
    }
    const tarballs = `${packMain.stdout}\n${packNative.stdout || ''}`
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.endsWith('.tgz'));
    if (tarballs.length < 2) {
      fail(`expected main+native tarballs, got: ${JSON.stringify(tarballs)}`);
    }
    const installRoot = mkdtempSync(join(tmpdir(), 'pdf-reader-local-install-'));
    try {
      const init = spawnSync('npm', ['init', '-y'], { cwd: installRoot, encoding: 'utf8' });
      if (init.status !== 0) fail(init.stderr || 'npm init failed');
      const tarballPaths = tarballs.map((name) => join(temp, name));
      const install = spawnSync('npm', ['install', ...tarballPaths, '--no-fund', '--no-audit'], {
        cwd: installRoot,
        encoding: 'utf8',
      });
      if (install.status !== 0) {
        fail(install.stderr || install.stdout || 'npm install local tarballs failed');
      }
      const nativeBinary = resolveNativeBinaryFromInstall(installRoot, platformId);
      if (!nativeBinary) {
        fail(`native binary missing after local tarball install for ${platformId}`);
      }
      const packageDir = join(installRoot, 'node_modules', '@sylphx', 'anymd');
      const installedPackage = JSON.parse(
        readFileSync(join(packageDir, 'package.json'), 'utf8')
      ) as { version?: string; bin?: Record<string, string> };
      const installedVersion = String(installedPackage.version ?? '');
      const bin = installedPackage.bin?.anymd ?? '';
      if (!installedVersion || !bin.includes('runtime-entry.js')) {
        fail('local install did not produce the versioned sole-Rust anymd launcher');
      }
      const nativePackageRoot = join(installRoot, 'node_modules', ...meta.npmName.split('/'));
      const nativeVersion = String(
        (
          JSON.parse(readFileSync(join(nativePackageRoot, 'package.json'), 'utf8')) as {
            version?: string;
          }
        ).version ?? ''
      );
      if (nativeVersion !== installedVersion) {
        fail(`local native version ${nativeVersion} != wrapper ${installedVersion}`);
      }
      const launcher = resolveInstalledLauncher(packageDir, bin);
      let initialize:
        | {
            serverName: string;
            serverVersion: string;
          }
        | undefined;
      if (!skipInitialize) {
        initialize = await mcpInitialize(
          launcher.command,
          launcher.args,
          installRoot,
          installedVersion
        );
      }
      const result = {
        ...plan,
        mode: 'local-pack',
        stagedBinary: staged,
        tarballs,
        nativeBinary,
        optionalPackageVersion: nativeVersion,
        installedLauncher: launcher.path,
        initialize: initialize ?? null,
        host: hostRuntimeFacts(),
        hostMatchesResolvedPlatformId: hostMatchesPlatformId(platformId),
        runtimeProofValid: hostMatchesPlatformId(platformId),
        note: 'Local pack + install + pure-Rust MCP initialize on matching host. Not npm registry readback.',
        pass: true,
      };
      console.log(JSON.stringify(result, null, 2));
    } finally {
      if (!keepTemp) rmSync(installRoot, { recursive: true, force: true });
    }
  } finally {
    if (!keepTemp) rmSync(temp, { recursive: true, force: true });
  }
  process.exit(0);
}

// registry mode
if (!versionArg) {
  fail('--registry requires --version=<published-version>', 2);
}

const temp = mkdtempSync(join(tmpdir(), 'pdf-reader-registry-install-'));
try {
  const init = spawnSync('npm', ['init', '-y'], { cwd: temp, encoding: 'utf8' });
  if (init.status !== 0) fail(init.stderr || 'npm init failed');

  let upgradeFrom: string | null = null;
  if (fromVersionArg && fromVersionArg !== versionArg) {
    const previousInstall = spawnSync(
      'npm',
      ['install', `@sylphx/anymd@${fromVersionArg}`, '--no-fund', '--no-audit'],
      { cwd: temp, encoding: 'utf8', env: process.env }
    );
    if (previousInstall.status !== 0) {
      fail(previousInstall.stderr || previousInstall.stdout || 'previous-version install failed');
    }
    const previousPackage = JSON.parse(
      readFileSync(join(temp, 'node_modules', '@sylphx', 'anymd', 'package.json'), 'utf8')
    ) as { version?: string };
    if (previousPackage.version !== fromVersionArg) {
      fail(
        `installed previous version ${String(previousPackage.version)} != requested ${fromVersionArg}`
      );
    }
    upgradeFrom = fromVersionArg;
  }

  const install = spawnSync(
    'npm',
    ['install', `@sylphx/anymd@${versionArg}`, '--no-fund', '--no-audit'],
    { cwd: temp, encoding: 'utf8', env: process.env }
  );
  if (install.status !== 0) {
    fail(install.stderr || install.stdout || 'npm install failed');
  }

  const pkgDir = join(temp, 'node_modules', '@sylphx', 'anymd');
  if (!existsSync(pkgDir)) fail('installed package directory missing');

  const mainPkg = JSON.parse(readFileSync(join(pkgDir, 'package.json'), 'utf8')) as {
    version?: string;
    bin?: Record<string, string>;
    exports?: Record<string, string>;
    optionalDependencies?: Record<string, string>;
  };
  if (mainPkg.version !== versionArg) {
    fail(`installed version ${mainPkg.version} != requested ${versionArg}`);
  }
  const bin = mainPkg.bin?.anymd ?? '';
  const soleRuntime =
    bin.includes('runtime-entry.js') ||
    String(mainPkg.exports?.['.'] ?? '').includes('runtime-entry.js');
  if (soleRuntime) {
    if (!bin.includes('runtime-entry.js')) {
      fail(`sole-runtime default bin must be runtime-entry.js; got ${bin}`);
    }
    if (mainPkg.exports?.['./typescript']) {
      fail('sole-Rust package must not export ./typescript');
    }
  } else if (!bin.includes('dist/index.js')) {
    fail(`default bin must be TypeScript dist/index.js or sole-runtime runtime-entry.js; got ${bin}`);
  }
  if (!(mainPkg.exports?.['./pure-rust'] ?? '').includes('pure-rust')) {
    fail('published package must expose ./pure-rust export');
  }

  const meta = NATIVE_PLATFORM_PACKAGES[platformId];
  const expectedOptional = mainPkg.optionalDependencies?.[meta.npmName];
  if (expectedOptional !== versionArg) {
    fail(
      `optionalDependencies missing ${meta.npmName}@${versionArg}; got ${String(expectedOptional)}`
    );
  }

  const nativeBinary = resolveNativeBinaryFromInstall(temp, platformId);
  if (!nativeBinary) {
    fail(
      `optional native package binary missing for ${platformId} after npm install ${versionArg}`
    );
  }

  const nativePkgDir = join(temp, 'node_modules', ...meta.npmName.split('/'));
  const nativePkg = JSON.parse(readFileSync(join(nativePkgDir, 'package.json'), 'utf8')) as {
    version?: string;
  };
  if (nativePkg.version !== versionArg) {
    fail(`installed native version ${String(nativePkg.version)} != requested ${versionArg}`);
  }
  const launcher = resolveInstalledLauncher(pkgDir, bin);

  let initialize:
    | {
        serverName: string;
        serverVersion: string;
      }
    | undefined;
  if (!skipInitialize) {
    initialize = await mcpInitialize(launcher.command, launcher.args, temp, versionArg);
  }

  const uninstall = spawnSync('npm', ['uninstall', '@sylphx/anymd', '--no-fund', '--no-audit'], {
    cwd: temp,
    encoding: 'utf8',
    env: process.env,
  });
  if (uninstall.status !== 0) {
    fail(uninstall.stderr || uninstall.stdout || 'npm uninstall failed');
  }
  const launcherShim = join(temp, 'node_modules', '.bin', 'anymd');
  if (
    existsSync(pkgDir) ||
    existsSync(nativePkgDir) ||
    existsSync(launcherShim) ||
    existsSync(`${launcherShim}.cmd`) ||
    existsSync(`${launcherShim}.ps1`)
  ) {
    fail('npm uninstall left the anymd package or launcher shim installed');
  }

  console.log(
    JSON.stringify(
      {
        ...plan,
        mode: 'registry',
        version: versionArg,
        upgradeFrom,
        installedPath: pkgDir,
        platformId,
        host: hostRuntimeFacts(),
        hostMatchesResolvedPlatformId: hostMatchesPlatformId(platformId),
        optionalPackage: meta.npmName,
        optionalPackageVersion: nativePkg.version,
        nativeBinary,
        installedLauncher: launcher.path,
        initialize: initialize ?? null,
        defaultBin: mainPkg.bin?.anymd ?? null,
        pureRustExport: mainPkg.exports?.['./pure-rust'] ?? null,
        lifecycle: {
          cleanInstall: true,
          upgrade: upgradeFrom ? { from: upgradeFrom, to: versionArg, pass: true } : null,
          uninstall: { pass: true },
        },
        pass: true,
        runtimeProofValid: hostMatchesPlatformId(platformId),
        note: !hostMatchesPlatformId(platformId)
          ? `Installed and executed native package for resolved host ${platformId}, but this does NOT prove a different matrix label if runner arch differs.`
          : skipInitialize
            ? 'Install + optional native binary presence only.'
            : 'Registry install + exact optional native + installed anymd launcher MCP initialize succeeded on matching host platform.',
      },
      null,
      2
    )
  );
} finally {
  if (!keepTemp) rmSync(temp, { recursive: true, force: true });
  else console.error(`[registry-install-proof] kept temp at ${temp}`);
}
