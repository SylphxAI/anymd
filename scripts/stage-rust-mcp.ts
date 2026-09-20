import fs from 'node:fs';
import path from 'node:path';
import { resolveCargoReleaseDir } from './native/cargo-target-dir.ts';
import {
  nativeBinaryRelativePath,
  resolveNativePlatformId,
} from './native/platform-package-map.ts';

const repoRoot = path.resolve(import.meta.dirname, '..');
const platformIdEarly = resolveNativePlatformId();
const binaryName =
  platformIdEarly != null
    ? path.basename(nativeBinaryRelativePath(platformIdEarly))
    : process.platform === 'win32'
      ? 'citra-mcp-server.exe'
      : 'citra-mcp-server';
const source = path.join(resolveCargoReleaseDir(), binaryName);
const legacyTargetDir = path.join(repoRoot, 'bin/native');
const legacyTarget = path.join(legacyTargetDir, binaryName);

if (!fs.existsSync(source)) {
  console.error(
    `[stage-rust-mcp] Missing release binary at ${source}. Run: bun run build:rust`
  );
  process.exit(1);
}

const stageCopy = (target: string): void => {
  fs.mkdirSync(path.dirname(target), { recursive: true });
  const tmp = `${target}.${process.pid}.tmp`;
  try {
    fs.copyFileSync(source, tmp);
    fs.chmodSync(tmp, 0o755);
    fs.renameSync(tmp, target);
  } catch (error) {
    try {
      fs.unlinkSync(tmp);
    } catch {
      // best-effort cleanup
    }
    throw error;
  }
};

// Compatibility path retained for existing local tooling.
stageCopy(legacyTarget);
console.log(`[stage-rust-mcp] Staged ${legacyTarget}`);

const platformId = resolveNativePlatformId();
if (platformId) {
  const platformTarget = path.join(repoRoot, nativeBinaryRelativePath(platformId));
  stageCopy(platformTarget);
  console.log(`[stage-rust-mcp] Staged platform path ${platformTarget}`);

  // Also stage into the optional package bin/ for local packaging smoke.
  const packageBinary = path.join(
    repoRoot,
    `packages/citra-${platformId}/bin`,
    path.basename(platformTarget)
  );
  stageCopy(packageBinary);
  console.log(`[stage-rust-mcp] Staged package path ${packageBinary}`);
} else {
  console.warn(
    `[stage-rust-mcp] Host ${process.platform}/${process.arch} has no optional native package mapping yet`
  );
}
