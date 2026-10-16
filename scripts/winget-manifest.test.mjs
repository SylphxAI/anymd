// Renders the winget manifests from a fixture SHA256SUMS.
import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
  parseSums,
  packageIdentifier,
  installerManifest,
  localeManifest,
  versionManifest,
} from "./winget-manifest.mjs";

const h = (c) => c.repeat(64);
const FIXTURE = [
  `${h("a")}  anymd-darwin-arm64.tar.gz`,
  `${h("e")} *anymd-win32-x64-msvc.zip`,
  "",
].join("\n");
const meta = {
  name: "anymd",
  version: "8.5.4",
  repo: "SylphxAI/anymd",
  description: 'Any file to "clean" Markdown',
  homepage: "https://sylphxai.github.io/anymd/",
  license: "MIT",
  publisher: "Sylphx",
};
const sums = parseSums(FIXTURE);
const input = { ...meta, sums };

test("parseSums reads the Windows archive", () => {
  assert.equal(sums["anymd-win32-x64-msvc.zip"], h("e"));
});

test("package identifier is Publisher.Name", () => {
  assert.equal(packageIdentifier("Sylphx", "anymd"), "Sylphx.anymd");
  assert.throws(() => packageIdentifier("---", "anymd"), /Publisher\.Package/);
});

test("installer points at the portable exe inside the release zip", () => {
  const text = installerManifest(input);
  assert.match(text, /^PackageIdentifier: Sylphx\.anymd$/m);
  assert.match(text, /^PackageVersion: 8\.5\.4$/m);
  assert.match(text, /^InstallerType: zip$/m);
  assert.match(text, /^NestedInstallerType: portable$/m);
  assert.match(text, /RelativeFilePath: anymd\.exe/);
  assert.match(text, /PortableCommandAlias: anymd/);
  assert.doesNotMatch(text, /^Commands:/m);
  assert.match(
    text,
    /InstallerUrl: https:\/\/github\.com\/SylphxAI\/anymd\/releases\/download\/v8\.5\.4\/anymd-win32-x64-msvc\.zip/,
  );
  assert.match(text, new RegExp(`InstallerSha256: ${"E".repeat(64)}`));
  assert.match(text, /^ManifestType: installer$/m);
  assert.match(text, /^ManifestVersion: 1\.12\.0$/m);
});

test("installer refuses a missing archive digest", () => {
  const { ["anymd-win32-x64-msvc.zip"]: _, ...rest } = sums;
  assert.throws(() => installerManifest({ ...input, sums: rest }), /anymd-win32-x64-msvc\.zip/);
});

test("a long description is shortened to the winget limit", () => {
  const locale = localeManifest({ ...input, description: "word ".repeat(80) });
  const line = locale.split("\n").find((l) => l.startsWith("ShortDescription:"));
  const value = JSON.parse(line.slice("ShortDescription: ".length));
  assert.ok(value.length <= 256, value.length);
  assert.ok(value.endsWith("…"));
});

test("locale and version manifests name the package", () => {
  assert.match(versionManifest(input), /^DefaultLocale: en-US$/m);
  const locale = localeManifest(input);
  assert.match(locale, /^Publisher: Sylphx$/m);
  assert.match(locale, /ShortDescription: "Any file to \\"clean\\" Markdown"/);
  assert.match(locale, /Description: "Any file to \\"clean\\" Markdown"/);
  assert.ok(!locale.split("\n").some((line) => line.startsWith("ShortDescription:") && line.length > 280));
  assert.match(locale, /^License: MIT$/m);
  assert.match(locale, /LicenseUrl: https:\/\/github\.com\/SylphxAI\/anymd\/blob\/v8\.5\.4\/LICENSE/);
  assert.match(locale, /^Moniker: anymd$/m);
});

test("CLI writes the three manifest files", () => {
  const d = mkdtempSync(join(tmpdir(), "winget-"));
  writeFileSync(join(d, "SHA256SUMS"), FIXTURE);
  writeFileSync(
    join(d, "package.json"),
    JSON.stringify({ description: "From npm", license: "MIT", homepage: "https://example.com" }),
  );
  execFileSync("node", [
    resolve(import.meta.dirname, "winget-manifest.mjs"),
    "--name",
    "anymd",
    "--version",
    "8.5.4",
    "--repo",
    "SylphxAI/anymd",
    "--sums",
    join(d, "SHA256SUMS"),
    "--package-json",
    join(d, "package.json"),
    "--out",
    join(d, "out"),
  ]);
  const installer = readFileSync(join(d, "out/Sylphx.anymd.installer.yaml"), "utf8");
  assert.match(installer, /RelativeFilePath: anymd\.exe/);
  const locale = readFileSync(join(d, "out/Sylphx.anymd.locale.en-US.yaml"), "utf8");
  assert.match(locale, /ShortDescription: "From npm"/);
  assert.match(locale, /PackageUrl: https:\/\/example\.com/);
  assert.match(readFileSync(join(d, "out/Sylphx.anymd.yaml"), "utf8"), /^ManifestType: version$/m);
});
