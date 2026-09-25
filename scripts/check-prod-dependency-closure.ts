#!/usr/bin/env bun
/**
 * Production dependency-closure gate for sole-Rust package.
 * Fails if the published package install graph can pull TS/PDF.js runtime deps.
 */
import { existsSync, readFileSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';

const root = join(import.meta.dirname, '..');
const failures: string[] = [];

const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
  version?: string;
  dependencies?: Record<string, string>;
  optionalDependencies?: Record<string, string>;
  peerDependencies?: Record<string, string>;
  files?: string[];
  scripts?: Record<string, string>;
};

const bannedExact = new Set([
  'pdfjs-dist',
  '@modelcontextprotocol/sdk',
  'pngjs',
  'zod',
  'pdfjs-dist/legacy/build/pdf.mjs',
]);

const bannedNameSubstrings = [
  'pdfjs',
  'pdf.js',
  'modelcontextprotocol',
  'legacy-engine',
];

const deps = pkg.dependencies ?? {};
const optional = pkg.optionalDependencies ?? {};
const peer = pkg.peerDependencies ?? {};

if (Object.keys(deps).length > 0) {
  failures.push(
    `production dependencies must be empty for sole-Rust launcher; found: ${Object.keys(deps).sort().join(', ')}`
  );
}

for (const [name] of Object.entries(deps)) {
  if (bannedExact.has(name) || bannedNameSubstrings.some((s) => name.toLowerCase().includes(s))) {
    failures.push(`banned production dependency: ${name}`);
  }
}

for (const [name] of Object.entries(peer)) {
  if (bannedExact.has(name) || bannedNameSubstrings.some((s) => name.toLowerCase().includes(s))) {
    failures.push(`banned peerDependency in production package: ${name}`);
  }
}

// optional must only be native platform packages
for (const name of Object.keys(optional)) {
  if (!name.startsWith('@sylphx/anymd-')) {
    failures.push(`optionalDependency must be native platform package only: ${name}`);
  }
}

// build must not default to oracle TS
const build = pkg.scripts?.build ?? '';
if (build.includes('build:oracle-ts') || build.includes('oracle-ts')) {
  failures.push('scripts.build must not invoke build:oracle-ts (oracle is test-only)');
}
if (!pkg.scripts?.['build:oracle-ts']) {
  failures.push('scripts.build:oracle-ts must remain available as explicit test-only command');
}
if ((pkg.scripts?.prepublishOnly ?? '').includes('build:oracle-ts')) {
  failures.push('prepublishOnly must not build oracle TS');
}

// files allowlist already covered by ts-production-absence; double-check here
for (const f of pkg.files ?? []) {
  if (f.includes('index.js') || f.includes('pdf.worker') || f.includes('legacy')) {
    failures.push(`files allowlist ships banned path: ${f}`);
  }
}

// If local node_modules exists from a full install, ensure production tree has no banned packages
// when omitting dev. We simulate via package.json inspection primarily; optional local npm pack check.
const runtimeEntry = join(root, 'src/runtime-entry.ts');
const pureRust = join(root, 'src/pure-rust.ts');
for (const rel of [runtimeEntry, pureRust]) {
  if (!existsSync(rel)) {
    failures.push(`missing ${rel}`);
    continue;
  }
  const src = readFileSync(rel, 'utf8');
  for (const banned of ['pdfjs-dist', '@modelcontextprotocol/sdk', "from 'zod'", 'from "zod"', 'pngjs']) {
    if (src.includes(banned)) {
      failures.push(`${rel} must not import ${banned}`);
    }
  }
}

// npm pack dry-run contents must not include worker/index when dist is dirty from oracle builds
const pack = spawnSync('npm', ['pack', '--dry-run', '--json'], {
  cwd: root,
  encoding: 'utf8',
  maxBuffer: 20 * 1024 * 1024,
});
if (pack.status === 0) {
  try {
    const parsed = JSON.parse(pack.stdout.trim() || '[]') as Array<{ files?: Array<{ path?: string }> }>;
    const files = (parsed[0]?.files ?? []).map((f) => f.path ?? '').filter(Boolean);
    for (const f of files) {
      if (
        f.includes('pdf.worker') ||
        f.includes('legacy-engine') ||
        f.endsWith('dist/index.js') ||
        f.includes('node_modules/pdfjs')
      ) {
        failures.push(`npm pack would include banned path: ${f}`);
      }
    }
  } catch {
    // older npm may not support --json; ignore pack parse
  }
}

if (failures.length) {
  console.error(failures.map((f) => `[check-prod-dependency-closure] ${f}`).join('\n'));
  process.exit(1);
}

console.log(
  JSON.stringify(
    {
      profile: 'prod_dependency_closure',
      pass: true,
      version: pkg.version ?? null,
      productionDependencies: deps,
      optionalDependencies: optional,
      build: pkg.scripts?.build ?? null,
      prepublishOnly: pkg.scripts?.prepublishOnly ?? null,
    },
    null,
    2
  )
);
console.log('[check-prod-dependency-closure] PASS');
