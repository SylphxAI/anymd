#!/usr/bin/env node

// src/runtime-entry.ts
import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// src/native/platform-package-map.ts
var NATIVE_PLATFORM_PACKAGES = {
  "darwin-arm64": {
    npmName: "@sylphx/anymd-darwin-arm64",
    packageDir: "packages/anymd-darwin-arm64",
    binaryName: "anymd",
    os: "darwin",
    cpu: "arm64"
  },
  "darwin-x64": {
    npmName: "@sylphx/anymd-darwin-x64",
    packageDir: "packages/anymd-darwin-x64",
    binaryName: "anymd",
    os: "darwin",
    cpu: "x64"
  },
  "linux-arm64-gnu": {
    npmName: "@sylphx/anymd-linux-arm64-gnu",
    packageDir: "packages/anymd-linux-arm64-gnu",
    binaryName: "anymd",
    os: "linux",
    cpu: "arm64"
  },
  "linux-x64-gnu": {
    npmName: "@sylphx/anymd-linux-x64-gnu",
    packageDir: "packages/anymd-linux-x64-gnu",
    binaryName: "anymd",
    os: "linux",
    cpu: "x64"
  },
  "win32-x64-msvc": {
    npmName: "@sylphx/anymd-win32-x64-msvc",
    packageDir: "packages/anymd-win32-x64-msvc",
    binaryName: "anymd.exe",
    os: "win32",
    cpu: "x64"
  }
};
var resolveNativePlatformId = (platform = process.platform, arch = process.arch) => {
  if (platform === "darwin" && arch === "arm64")
    return "darwin-arm64";
  if (platform === "darwin" && arch === "x64")
    return "darwin-x64";
  if (platform === "linux" && arch === "arm64")
    return "linux-arm64-gnu";
  if (platform === "linux" && arch === "x64")
    return "linux-x64-gnu";
  if (platform === "win32" && arch === "x64")
    return "win32-x64-msvc";
  return null;
};
if (false) {}

// src/runtime-entry.ts
var require2 = createRequire(import.meta.url);
var here = dirname(fileURLToPath(import.meta.url));
var packageRoot = join(here, "..");
var packageVersion = String(JSON.parse(readFileSync(join(packageRoot, "package.json"), "utf8")).version ?? "");
var versionedPackageBinary = (nativePackageRoot, binaryName) => {
  const binary = join(nativePackageRoot, "bin", binaryName);
  if (!existsSync(binary))
    return null;
  try {
    const nativeVersion = String(JSON.parse(readFileSync(join(nativePackageRoot, "package.json"), "utf8")).version ?? "");
    if (nativeVersion !== packageVersion) {
      console.error(`[anymd] refusing native package version ${nativeVersion || "unknown"}; wrapper version is ${packageVersion || "unknown"}.`);
      return null;
    }
  } catch {
    return null;
  }
  return binary;
};
var resolveNativeBinary = () => {
  const platformId = resolveNativePlatformId();
  if (!platformId)
    return null;
  const meta = NATIVE_PLATFORM_PACKAGES[platformId];
  const candidates = [];
  const workspacePackage = versionedPackageBinary(join(packageRoot, meta.packageDir), meta.binaryName);
  if (workspacePackage)
    candidates.push(workspacePackage);
  candidates.push(join(packageRoot, "bin/native", platformId, meta.binaryName));
  const installedPackage = versionedPackageBinary(join(packageRoot, "node_modules", meta.npmName), meta.binaryName);
  if (installedPackage)
    candidates.push(installedPackage);
  try {
    const pkgJson = require2.resolve(`${meta.npmName}/package.json`, {
      paths: [packageRoot]
    });
    const resolvedPackage = versionedPackageBinary(dirname(pkgJson), meta.binaryName);
    if (resolvedPackage)
      candidates.push(resolvedPackage);
  } catch {}
  for (const candidate of candidates) {
    if (existsSync(candidate))
      return candidate;
  }
  return null;
};
if (process.env["PDF_READER_FORCE_TYPESCRIPT"] === "1" || process.env["PDF_READER_ENGINE_MODE"] === "typescript" || process.env["PDF_READER_ENGINE_MODE"] === "ts") {
  console.error([
    "[anymd] TypeScript production runtime has been removed from this package.",
    "Use the immutable historical LKG @sylphx/pdf-reader-mcp@3.0.14 for TypeScript rollback,",
    "or install/run the pure-Rust native binary for this package version."
  ].join(`
`));
  process.exit(2);
}
var nativeBinary = resolveNativeBinary();
if (!nativeBinary) {
  const platformId = resolveNativePlatformId();
  const platformLabel = platformId ?? `${process.platform}/${process.arch}`;
  console.error([
    `[anymd] pure-Rust native binary not found for ${platformLabel}.`,
    "anymd is sole-Rust: there is no bundled TypeScript PDF runtime.",
    "Install the matching optional native package at the same anymd version.",
    "Historical TypeScript LKG remains available only as @sylphx/pdf-reader-mcp@3.0.14 (external pin)."
  ].join(`
`));
  process.exit(1);
}
var child = spawn(nativeBinary, process.argv.slice(2), {
  stdio: "inherit",
  env: {
    ...process.env,
    PDF_READER_ENGINE_MODE: process.env["PDF_READER_ENGINE_MODE"] || "pure-rust",
    MCP_TRANSPORT: process.env["MCP_TRANSPORT"] || "stdio"
  }
});
child.once("error", (error) => {
  console.error(`[anymd] failed to start native server: ${error.message}`);
  process.exit(1);
});
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.once(signal, () => {
    if (!child.killed)
      child.kill(signal);
  });
}
child.on("exit", (code, signal) => {
  if (signal) {
    process.exit(128 + (signal === "SIGINT" ? 2 : signal === "SIGTERM" ? 15 : 1));
    return;
  }
  process.exit(code ?? 1);
});
