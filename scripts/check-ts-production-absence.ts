#!/usr/bin/env bun
/**
 * Mechanical sole-Rust production absence gate.
 * Fails if the published package surface still ships or exports TypeScript PDF runtime.
 */
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dirname, '..');
const failures: string[] = [];

const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
  version?: string;
  bin?: Record<string, string>;
  exports?: Record<string, string>;
  files?: string[];
  scripts?: Record<string, string>;
  dependencies?: Record<string, string>;
};

if (pkg.name !== '@sylphx/anymd') {
  failures.push('package name must be @sylphx/anymd (brand-sole)');
}
if (pkg.bin?.['anymd'] !== './dist/runtime-entry.js') {
  failures.push('bin anymd must be ./dist/runtime-entry.js');
}
if (pkg.exports?.['./typescript']) {
  failures.push('package.json must not export ./typescript in sole-Rust production');
}
const prodDeps = Object.keys(pkg.dependencies ?? {});
if (prodDeps.length > 0) {
  failures.push(`package.json production dependencies must be empty; found: ${prodDeps.join(', ')}`);
}
for (const banned of ['pdfjs-dist', '@modelcontextprotocol/sdk', 'pngjs', 'zod']) {
  if ((pkg.dependencies ?? {})[banned]) {
    failures.push(`package.json must not declare production dependency ${banned}`);
  }
}

if (pkg.bin?.anymd !== './dist/runtime-entry.js') {
  failures.push('bin anymd must be ./dist/runtime-entry.js');
}
if (pkg.exports?.['.'] !== './dist/runtime-entry.js') {
  failures.push('exports["."] must be ./dist/runtime-entry.js');
}

const files = pkg.files ?? [];
const shipsWholeDist = files.some((f) => f === 'dist' || f === 'dist/' || f === 'dist/**');
if (shipsWholeDist) {
  failures.push('package files must not ship entire dist/; allowlist sole-Rust launcher artifacts only');
}
for (const banned of ['dist/index.js', 'dist/legacy-engine-runtime.js', 'dist/pdf.worker.mjs', 'dist/pdf.worker.min.mjs', 'dist/doctor-cli.js']) {
  if (files.includes(banned)) {
    // explicit allow of banned path would be wrong
    failures.push(`package files must not include banned production payload ${banned}`);
  }
}

const runtime = readFileSync(join(root, 'src/runtime-entry.ts'), 'utf8');
if (runtime.includes('loadTypeScriptRuntime') || runtime.includes("join(here, 'index.js')")) {
  failures.push('src/runtime-entry.ts must not load TypeScript dist/index.js');
}
if (runtime.includes('await import(pathToFileURL') && runtime.includes('index.js')) {
  failures.push('src/runtime-entry.ts must not dynamic-import TypeScript entry');
}
if (!runtime.includes('TypeScript production runtime has been removed') && !runtime.includes('sole-Rust')) {
  failures.push('src/runtime-entry.ts must document sole-Rust / no TS production runtime');
}

// If dist exists, ensure package build outputs do not require banned files to be present for production.
// Presence of local oracle builds under dist/ is allowed in workspace, but package files allowlist must exclude them.
const bannedDist = [
  'dist/index.js',
  'dist/legacy-engine-runtime.js',
  'dist/pdf.worker.mjs',
  'dist/pdf.worker.min.mjs',
];
// Verify files allowlist only contains sole-rust paths under dist
for (const f of files) {
  if (f.startsWith('dist/') && !['dist/runtime-entry.js', 'dist/pure-rust.js'].includes(f) && f !== 'dist/native/') {
    if (f.includes('index') || f.includes('legacy') || f.includes('pdf.worker') || f.includes('doctor')) {
      failures.push(`files allowlist contains non-sole-Rust dist artifact: ${f}`);
    }
  }
}

const matrix = JSON.parse(
  readFileSync(join(root, 'docs/specs/pure-rust-capability-matrix.json'), 'utf8')
) as {
  productTruth?: {
    typescriptProductionShipped?: boolean;
    automaticTypescriptFallback?: boolean;
    pureRustStatus?: string;
    soleRustProduction?: boolean;
  };
};
if (matrix.productTruth?.typescriptProductionShipped !== false) {
  failures.push('productTruth.typescriptProductionShipped must be false');
}
if (matrix.productTruth?.automaticTypescriptFallback !== false) {
  failures.push('productTruth.automaticTypescriptFallback must be false');
}
if (matrix.productTruth?.soleRustProduction !== true) {
  failures.push('productTruth.soleRustProduction must be true for this gate');
}

if (failures.length) {
  console.error(failures.map((f) => `[check-ts-production-absence] ${f}`).join('\n'));
  process.exit(1);
}
console.log(
  JSON.stringify(
    {
      profile: 'ts_production_absence',
      pass: true,
      version: pkg.version ?? null,
      bin: pkg.bin?.anymd ?? null,
      exports: pkg.exports ?? null,
      files,
    },
    null,
    2
  )
);
console.log('[check-ts-production-absence] PASS');
