#!/usr/bin/env bun
/**
 * One product version in every manifest the release reads: the main npm
 * package and its optional dependencies, the platform packages, the alias
 * packages and their pin on @sylphx/anymd, server.json, and the Cargo
 * workspace (which the binary reports as its version).
 *
 *   bun scripts/set-version.ts 8.1.0   # set it everywhere, then refresh Cargo.lock
 *   bun scripts/set-version.ts --check # fail when any manifest disagrees
 */
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dirname, '..');
const MAIN = 'packages/anymd/package.json';
const CARGO = 'Cargo.toml';
const CARGO_VERSION = /(\[workspace\.package\][^[]*?\nversion = ")([^"]+)(")/;

type Json = Record<string, unknown> & { version: string };
type Manifest = {
  path: string;
  versions: (json: Json) => string[];
  set: (json: Json, v: string) => void;
};

const pins = (deps: unknown) => Object.values((deps ?? {}) as Record<string, string>);
const repin = (deps: unknown, v: string, only?: string) => {
  const map = deps as Record<string, string>;
  for (const name of Object.keys(map)) if (!only || name === only) map[name] = v;
};
const dirs = (parent: string) =>
  readdirSync(join(root, parent)).map((d) => `${parent}/${d}/package.json`);

const manifests: Manifest[] = [
  {
    path: MAIN,
    versions: (j) => [j.version, ...pins(j['optionalDependencies'])],
    set: (j, v) => {
      j.version = v;
      repin(j['optionalDependencies'], v);
    },
  },
  ...dirs('packages/npm').map((path) => ({
    path,
    versions: (j: Json) => [j.version],
    set: (j: Json, v: string) => {
      j.version = v;
    },
  })),
  ...dirs('packages/aliases').map((path) => ({
    path,
    versions: (j: Json) => [
      j.version,
      (j['dependencies'] as Record<string, string>)['@sylphx/anymd'] ?? '',
    ],
    set: (j: Json, v: string) => {
      j.version = v;
      repin(j['dependencies'], v, '@sylphx/anymd');
    },
  })),
  {
    path: 'server.json',
    versions: (j) => [j.version, ...(j['packages'] as Json[]).map((p) => p.version)],
    set: (j, v) => {
      j.version = v;
      for (const p of j['packages'] as Json[]) p.version = v;
    },
  },
];

const read = (path: string) => readFileSync(join(root, path), 'utf8');
const readJson = (path: string) => JSON.parse(read(path)) as Json;
const cargoVersion = () => CARGO_VERSION.exec(read(CARGO))?.[2];

if (process.argv[2] === '--check') {
  const want = readJson(MAIN).version;
  const bad = manifests.flatMap((m) =>
    m
      .versions(readJson(m.path))
      .filter((got) => got !== want)
      .map((got) => `${m.path}: ${got}`)
  );
  if (cargoVersion() !== want) bad.push(`${CARGO} [workspace.package]: ${cargoVersion()}`);
  if (bad.length) {
    console.error(`[set-version] want ${want} everywhere:\n  ${bad.join('\n  ')}`);
    process.exit(1);
  }
  console.log(`[set-version] every manifest is at ${want}`);
} else {
  const v = process.argv[2] ?? '';
  if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(v)) {
    console.error('usage: bun scripts/set-version.ts <X.Y.Z> | --check');
    process.exit(2);
  }
  for (const m of manifests) {
    const json = readJson(m.path);
    m.set(json, v);
    writeFileSync(join(root, m.path), `${JSON.stringify(json, null, 2)}\n`);
  }
  writeFileSync(join(root, CARGO), read(CARGO).replace(CARGO_VERSION, `$1${v}$3`));
  console.log(
    `[set-version] ${v}; now run \`cargo update -w\` and add a "## ${v}" section to CHANGELOG.md`
  );
}
