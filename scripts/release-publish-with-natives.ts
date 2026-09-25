#!/usr/bin/env bun
/**
 * Publish five optional native packages (when binaries are staged), then the main package,
 * then the compatibility alias packages (@sylphx/citra, @sylphx/pdf-reader-mcp) that
 * depend on the main package at the exact same version.
 *
 * Guards:
 * - verified-candidate admission must pass
 * - dropInFor3014 must remain false for this progress publish path unless explicitly overridden
 * - each native package prepublishOnly refuses empty binaries
 *
 * Usage:
 *   bun scripts/release-publish-with-natives.ts
 *   bun scripts/release-publish-with-natives.ts --dry-run
 *   bun scripts/release-publish-with-natives.ts --skip-main
 *   bun scripts/release-publish-with-natives.ts --main-only
 *   bun scripts/release-publish-with-natives.ts --skip-aliases
 */
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { NATIVE_PLATFORM_PACKAGES } from '../src/native/platform-package-map.ts';
import { ALIAS_PACKAGES, MAIN_PACKAGE } from './sync-alias-packages.ts';

const root = join(import.meta.dirname, '..');
const dryRun = process.argv.includes('--dry-run');
const skipMain = process.argv.includes('--skip-main');
const mainOnly = process.argv.includes('--main-only');
const skipAliases = process.argv.includes('--skip-aliases');

const run = (cmd: string, args: string[], cwd = root, options?: { allowDryRunSkip?: boolean }) => {
  console.log(`[release-publish-with-natives] $ ${cmd} ${args.join(' ')}`);
  if (dryRun && options?.allowDryRunSkip) {
    console.log('[release-publish-with-natives] dry-run: skipped publish side effect');
    return;
  }
  const result = spawnSync(cmd, args, { cwd, stdio: 'inherit', env: process.env });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
};

const packageIdentity = (cwd: string): { name: string; version: string } => {
  const manifest = JSON.parse(readFileSync(join(cwd, 'package.json'), 'utf8')) as {
    name?: string;
    version?: string;
  };
  if (!manifest.name || !manifest.version) {
    throw new Error(`package identity missing in ${cwd}`);
  }
  return { name: manifest.name, version: manifest.version };
};

const localTarballIntegrity = (cwd: string): string => {
  const destination = mkdtempSync(join(tmpdir(), 'anymd-publish-integrity-'));
  try {
    const packed = spawnSync('npm', ['pack', '--json', '--pack-destination', destination], {
      cwd,
      encoding: 'utf8',
      env: process.env,
    });
    if (packed.status !== 0) {
      throw new Error(packed.stderr || packed.stdout || `npm pack failed in ${cwd}`);
    }
    const result = JSON.parse(packed.stdout) as Array<{ integrity?: string }>;
    const integrity = result[0]?.integrity;
    if (!integrity) throw new Error(`npm pack returned no integrity in ${cwd}`);
    return integrity;
  } finally {
    rmSync(destination, { recursive: true, force: true });
  }
};

/**
 * npm can only build a sigstore provenance bundle on a GitHub-hosted runner.
 * On a self-hosted runner `--provenance` fails the publish with
 * E422 "Unsupported GitHub Actions runner environment", which silently blocks
 * every release. This fleet is self-hosted (sylphx-linux-standard), so ask for
 * provenance only where npm can produce it; otherwise publish without the
 * bundle and record the absent attestation instead of claiming one.
 */
const provenanceArgs = (): string[] => {
  const override = process.env['ANYMD_NPM_PROVENANCE']?.trim().toLowerCase();
  if (override === '1' || override === 'true') return ['--provenance'];
  if (override === '0' || override === 'false') return [];
  const runnerEnv = process.env['RUNNER_ENVIRONMENT']?.trim().toLowerCase();
  const inActions = process.env['GITHUB_ACTIONS'] === 'true';
  // Absent Actions context (a maintainer shell) cannot attest anywhere.
  if (!inActions) return [];
  return runnerEnv === 'github-hosted' ? ['--provenance'] : [];
};

const publishArgs = (): string[] => ['publish', '--access', 'public', ...provenanceArgs()];

const publishOrVerify = (cwd: string) => {
  if (dryRun) {
    run('npm', publishArgs(), cwd, {
      allowDryRunSkip: true,
    });
    return;
  }

  const { name, version } = packageIdentity(cwd);
  const spec = `${name}@${version}`;
  const registry = spawnSync('npm', ['view', spec, 'dist.integrity', '--json'], {
    cwd,
    encoding: 'utf8',
    env: process.env,
  });
  if (registry.status === 0 && registry.stdout.trim()) {
    const registryIntegrity = String(JSON.parse(registry.stdout));
    const localIntegrity = localTarballIntegrity(cwd);
    if (registryIntegrity !== localIntegrity) {
      console.error(
        `[release-publish-with-natives] immutable ${spec} already exists with different integrity: registry=${registryIntegrity} local=${localIntegrity}`
      );
      process.exit(1);
    }
    console.log(
      `[release-publish-with-natives] ${spec} already published with matching integrity; skipping immutable republish`
    );
    return;
  }

  const args = publishArgs();
  console.log(
    `[release-publish-with-natives] npm ${args.join(' ')}` +
      (args.includes('--provenance')
        ? ''
        : ' (no provenance bundle: not a github-hosted runner)')
  );
  run('npm', args, cwd);
};

// A publish command must be bound to the reviewed release commit. Non-publishing
// trunk checks may use the looser admission mode, but this mutation path may not.
run('bun', ['scripts/check-verified-candidate-admission.ts', '--require-exact-head']);

const matrix = JSON.parse(
  readFileSync(join(root, 'docs/specs/pure-rust-capability-matrix.json'), 'utf8')
) as { productTruth?: { dropInFor3014?: boolean; publishFreeze?: boolean; version?: string } };
// dropInFor3014=true is allowed for sole-runtime default publishes after
// check-verified-candidate-admission passes (requires soleRuntimeAuthorized).
if (matrix.productTruth?.dropInFor3014 === true) {
  console.log(
    '[release-publish-with-natives] sole-runtime publish path (dropInFor3014=true)'
  );
}

run('bun', ['scripts/native/sync-native-package-manifests.ts']);
// Aliases must already be in lockstep in the reviewed commit (release-version.sh syncs them).
run('bun', ['scripts/sync-alias-packages.ts', '--check']);

if (!mainOnly) {
  // Require all five binaries for a full native publish.
  // Dry-run without staged cross-platform binaries validates manifests only.
  if (dryRun) {
    run('bun', ['scripts/native/assert-native-packages-ready.ts', '--manifests-only', '--all']);
  } else {
    run('bun', ['scripts/native/assert-native-packages-ready.ts', '--all']);
  }
  for (const meta of Object.values(NATIVE_PLATFORM_PACKAGES)) {
    const cwd = join(root, meta.packageDir);
    if (!existsSync(join(cwd, 'package.json'))) {
      console.error(`missing ${cwd}/package.json`);
      process.exit(1);
    }
    publishOrVerify(cwd);
  }
}

if (!skipMain) {
  publishOrVerify(root);
  run('npm', ['view', MAIN_PACKAGE, 'version']);
}

// Aliases depend on the main package at the exact version, so they go last.
if (!skipMain && !skipAliases) {
  for (const alias of ALIAS_PACKAGES) {
    publishOrVerify(join(root, alias.packageDir));
  }
}

console.log(
  JSON.stringify(
    {
      profile: 'release_publish_with_natives',
      pass: true,
      dryRun,
      skipMain,
      mainOnly,
      skipAliases,
      aliases: skipMain || skipAliases ? [] : ALIAS_PACKAGES.map((alias) => alias.npmName),
      publishFreeze: matrix.productTruth?.publishFreeze ?? null,
      dropInFor3014: matrix.productTruth?.dropInFor3014 ?? null,
    },
    null,
    2
  )
);
console.log('[release-publish-with-natives] PASS');
