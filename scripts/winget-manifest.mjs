// Renders the Windows Package Manager manifests for the portable Windows
// binary attached to a GitHub release. The archive is the one release.yml
// publishes: <name>-win32-x64-msvc.zip, containing <name>.exe at the root.
// Usage: node winget-manifest.mjs --name N --version V --repo owner/repo
//   --sums SHA256SUMS [--package-json F] [--description D] [--homepage H]
//   [--license L] [--publisher P] [--out DIR]
// SHA256SUMS is `sha256sum` output. Without --out the three YAML files are
// written to stdout, separated by a form feed.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const WIN_PLATFORM = "win32-x64-msvc";
const MANIFEST_VERSION = "1.10.0";

export function parseSums(text) {
  const sums = {};
  for (const line of text.split("\n")) {
    const m = line.match(/^([a-f0-9]{64}) [ *](.+)$/);
    if (m) sums[m[2].trim()] = m[1];
  }
  return sums;
}

const yaml = (s) => JSON.stringify(String(s));
const SHORT_LIMIT = 256;

function shorten(text) {
  const oneLine = String(text).replace(/\s+/g, " ").trim();
  if (oneLine.length <= SHORT_LIMIT) return oneLine;
  const cut = oneLine.slice(0, SHORT_LIMIT - 1);
  const at = cut.lastIndexOf(" ");
  return `${(at > 40 ? cut.slice(0, at) : cut).trimEnd()}…`;
}
const releaseUrl = (repo, version, file) =>
  `https://github.com/${repo}/releases/download/v${version}/${file}`;

// Publisher is capitalised; the package keeps the binary's own name, so the
// install command stays `winget install Sylphx.anymd`.
export function packageIdentifier(publisher, name) {
  const publisherPart = publisher
    .split(/[^A-Za-z0-9]+/)
    .filter(Boolean)
    .map((p) => p[0].toUpperCase() + p.slice(1))
    .join("");
  const namePart = name.replace(/[^A-Za-z0-9]+/g, "");
  const id = `${publisherPart}.${namePart}`;
  if (!/^[A-Za-z0-9]+(\.[A-Za-z0-9]+){1,3}$/.test(id)) {
    throw new Error(`package identifier ${id} is not Publisher.Package`);
  }
  return id;
}

const SCHEMA = {
  version: "version",
  installer: "installer",
  // The published schema URL lowercases this one word: aka.ms/winget-manifest.defaultlocale.1.10.0.schema.json
  defaultLocale: "defaultlocale",
};

function header(kind) {
  return [
    `# yaml-language-server: $schema=https://aka.ms/winget-manifest.${SCHEMA[kind]}.${MANIFEST_VERSION}.schema.json`,
    "",
  ].join("\n");
}

function meta(input) {
  const { name, version, repo, sums, description, homepage, license, publisher } = input;
  if (!name || !version || !/^[^/]+\/[^/]+$/.test(repo ?? "")) {
    throw new Error("--name, --version and --repo owner/repo are required");
  }
  if (!publisher) throw new Error("--publisher is required");
  const file = `${name}-${WIN_PLATFORM}.zip`;
  const sha = sums[file];
  if (!sha) throw new Error(`no SHA-256 for ${file}`);
  return {
    name,
    version,
    repo,
    file,
    sha,
    description,
    homepage,
    license,
    publisher,
    id: packageIdentifier(publisher, name),
  };
}

export function versionManifest(input) {
  const m = meta(input);
  return (
    header("version") +
    [
      `PackageIdentifier: ${m.id}`,
      `PackageVersion: ${m.version}`,
      "DefaultLocale: en-US",
      "ManifestType: version",
      `ManifestVersion: ${MANIFEST_VERSION}`,
      "",
    ].join("\n")
  );
}

export function installerManifest(input) {
  const m = meta(input);
  return (
    header("installer") +
    [
      `PackageIdentifier: ${m.id}`,
      `PackageVersion: ${m.version}`,
      "InstallerType: zip",
      "NestedInstallerType: portable",
      "UpgradeBehavior: uninstallPrevious",
      "Commands:",
      `  - ${m.name}`,
      "Installers:",
      "  - Architecture: x64",
      "    NestedInstallerFiles:",
      `      - RelativeFilePath: ${m.name}.exe`,
      `        PortableCommandAlias: ${m.name}`,
      `    InstallerUrl: ${releaseUrl(m.repo, m.version, m.file)}`,
      `    InstallerSha256: ${m.sha.toUpperCase()}`,
      "ManifestType: installer",
      `ManifestVersion: ${MANIFEST_VERSION}`,
      "",
    ].join("\n")
  );
}

export function localeManifest(input) {
  const m = meta(input);
  const owner = m.repo.split("/")[0];
  return (
    header("defaultLocale") +
    [
      `PackageIdentifier: ${m.id}`,
      `PackageVersion: ${m.version}`,
      "PackageLocale: en-US",
      `Publisher: ${m.publisher}`,
      `PublisherUrl: https://github.com/${owner}`,
      `PublisherSupportUrl: https://github.com/${m.repo}/issues`,
      `PackageName: ${m.name}`,
      `PackageUrl: ${m.homepage}`,
      `License: ${m.license || "MIT"}`,
      `LicenseUrl: https://github.com/${m.repo}/blob/v${m.version}/LICENSE`,
      `ShortDescription: ${yaml(shorten(m.description))}`,
      `Description: ${yaml(m.description)}`,
      `Moniker: ${m.name}`,
      "Tags:",
      "  - markdown",
      "  - mcp",
      "  - cli",
      "  - pdf",
      "ManifestType: defaultLocale",
      `ManifestVersion: ${MANIFEST_VERSION}`,
      "",
    ].join("\n")
  );
}

export function manifests(input) {
  return {
    version: versionManifest(input),
    installer: installerManifest(input),
    locale: localeManifest(input),
  };
}

function writeAll(dir, id, files) {
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, `${id}.yaml`), files.version);
  writeFileSync(join(dir, `${id}.installer.yaml`), files.installer);
  writeFileSync(join(dir, `${id}.locale.en-US.yaml`), files.locale);
}

function main(argv) {
  const a = {};
  for (let i = 0; i < argv.length; i += 2) a[argv[i].replace(/^--/, "")] = argv[i + 1] ?? "";
  const pkg = a["package-json"] ? JSON.parse(readFileSync(a["package-json"], "utf8")) : {};
  const input = {
    name: a.name,
    version: a.version,
    repo: a.repo,
    sums: parseSums(readFileSync(a.sums, "utf8")),
    description: a.description || pkg.description || `${a.name} CLI`,
    homepage:
      a.homepage ||
      (typeof pkg.homepage === "string" && pkg.homepage) ||
      `https://github.com/${a.repo}`,
    license: a.license || (typeof pkg.license === "string" && pkg.license) || "",
    publisher: a.publisher || "Sylphx",
  };
  const files = manifests(input);
  if (a.out) writeAll(a.out, packageIdentifier(input.publisher, input.name), files);
  else process.stdout.write(`${files.version}\f${files.installer}\f${files.locale}`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(`::error::${e.message}`);
    process.exit(1);
  }
}
