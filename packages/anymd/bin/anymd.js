#!/usr/bin/env node
// mcp-kit npm launcher: runs this package's native binary for the current
// platform. Copy it to <package>/bin/<name>.js unchanged; it reads the rest
// from package.json:
//   "bin": { "<name>": "bin/<name>.js" }       -> binary name
//   optionalDependencies "<package>-<platform>" -> platform packages
// Override the binary with <NAME>_BIN=/path/to/binary.
// anymd: mcp-kit's launcher plus the ANYMD_RUST_BIN override.
"use strict";
const { spawnSync } = require("node:child_process");
const { existsSync } = require("node:fs");
const path = require("node:path");

const pkg = require(path.join(__dirname, "..", "package.json"));
const name = Object.keys(pkg.bin)[0];
const PLATFORMS = {
  "darwin-arm64": "darwin-arm64",
  "darwin-x64": "darwin-x64",
  "linux-x64": "linux-x64-gnu",
  "linux-arm64": "linux-arm64-gnu",
  "win32-x64": "win32-x64-msvc",
};

function resolveBinary() {
  // anymd also reads ANYMD_RUST_BIN, its override name before mcp-kit.
  const override = process.env[`${name.toUpperCase().replace(/-/g, "_")}_BIN`] || process.env.ANYMD_RUST_BIN;
  if (override && existsSync(override)) return override;
  const exe = process.platform === "win32" ? `${name}.exe` : name;
  const key = PLATFORMS[`${process.platform}-${process.arch}`];
  if (key) {
    try {
      return require.resolve(`${pkg.name}-${key}/${exe}`);
    } catch {}
  }
  // Development checkout: <repo>/packages/<pkg>/bin -> <repo>/target/.
  const root = path.resolve(__dirname, "..", "..", "..");
  for (const p of [path.join(root, "target", "release", exe), path.join(root, "target", "debug", exe)]) {
    if (existsSync(p)) return p;
  }
  return null;
}

const bin = resolveBinary();
if (!bin) {
  console.error(
    `${name}: no native binary for ${process.platform}-${process.arch}.\n` +
      "Supported: macOS (arm64, x64), Linux glibc (x64, arm64), Windows x64.\n" +
      "If optional dependencies were skipped, reinstall without --no-optional."
  );
  process.exit(1);
}
const res = spawnSync(bin, process.argv.slice(2), { stdio: "inherit", windowsHide: true });
if (res.error) {
  console.error(`${name}: failed to start ${bin}: ${res.error.message}`);
  process.exit(1);
}
process.exit(res.status === null ? 1 : res.status);
