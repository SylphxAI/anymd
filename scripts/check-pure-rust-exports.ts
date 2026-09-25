#!/usr/bin/env bun
/** Pure-Rust npm package export contract checks (sole-Rust production under ADR-0006). */
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const root = join(import.meta.dirname, '..');
const failures: string[] = [];

const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as {
  exports?: Record<string, string>;
  bin?: Record<string, string>;
  files?: string[];
  scripts?: Record<string, string>;
};
const matrix = JSON.parse(
  readFileSync(join(root, 'docs/specs/pure-rust-capability-matrix.json'), 'utf8')
) as {
  productTruth?: {
    dropInFor3014?: boolean;
    publishFreeze?: boolean;
    soleRustProduction?: boolean;
    typescriptProductionShipped?: boolean;
  };
  claimedForDifferential?: string[];
};

const defaultBin = pkg.bin?.anymd;
const defaultExport = pkg.exports?.['.'];
const sole = matrix.productTruth?.soleRustProduction === true;

if (sole || matrix.productTruth?.dropInFor3014 === true) {
  if (defaultBin !== './dist/runtime-entry.js') {
    failures.push('default bin must be dist/runtime-entry.js for pure-Rust production package');
  }
  if (defaultExport !== './dist/runtime-entry.js') {
    failures.push('default exports["."] must be dist/runtime-entry.js for pure-Rust production package');
  }
} else {
  if (defaultBin !== './dist/index.js') {
    failures.push('default bin must remain TypeScript dist/index.js while pure-Rust is not production');
  }
  if (defaultExport !== './dist/index.js') {
    failures.push('default exports["."] must remain TypeScript dist/index.js while pure-Rust is not production');
  }
}

if (sole || matrix.productTruth?.typescriptProductionShipped === false) {
  if (pkg.exports?.['./typescript']) {
    failures.push('sole-Rust production must not export ./typescript');
  }
} else if (pkg.exports?.['./typescript'] !== './dist/index.js') {
  failures.push('exports["./typescript"] must keep TypeScript dist/index.js fallback while TS is shipped');
}

if (pkg.exports?.['./pure-rust'] !== './dist/pure-rust.js') {
  failures.push('exports["./pure-rust"] must point at dist/pure-rust.js');
}
if (!existsSync(join(root, 'src/pure-rust.ts'))) {
  failures.push('src/pure-rust.ts missing');
}
if (!existsSync(join(root, 'src/native/platform-package-map.ts'))) {
  failures.push('src/native/platform-package-map.ts missing');
}
if (!(pkg.scripts?.['build:package'] ?? pkg.scripts?.build ?? '').includes('src/pure-rust.ts')) {
  failures.push('build:package (or build) must compile src/pure-rust.ts');
}
const pureRustSource = readFileSync(join(root, 'src/pure-rust.ts'), 'utf8');
if (matrix.productTruth?.dropInFor3014 === true) {
  if (!pureRustSource.includes('dropInFor3014: true')) {
    failures.push('src/pure-rust.ts must set dropInFor3014: true when productTruth.dropInFor3014=true');
  }
} else if (matrix.productTruth?.dropInFor3014 === false) {
  if (!pureRustSource.includes('dropInFor3014: false')) {
    failures.push('src/pure-rust.ts must set dropInFor3014: false while productTruth.dropInFor3014=false');
  }
}
if (matrix.productTruth?.publishFreeze === false) {
  if (!pureRustSource.includes('publishFreeze: false')) {
    failures.push('src/pure-rust.ts publishFreeze must match productTruth.publishFreeze=false');
  }
} else if (matrix.productTruth?.publishFreeze === true) {
  if (!pureRustSource.includes('publishFreeze: true')) {
    failures.push('src/pure-rust.ts publishFreeze must match productTruth.publishFreeze=true');
  }
}
if (
  !matrix.claimedForDifferential?.some((entry) => entry.includes('pure-Rust npm library export'))
) {
  failures.push('matrix must claim pure-Rust npm library export contract honestly');
}

if (!existsSync(join(root, 'dist/pure-rust.js'))) {
  failures.push('dist/pure-rust.js missing; run bun run build:package');
}
if (!existsSync(join(root, 'dist/runtime-entry.js'))) {
  failures.push('dist/runtime-entry.js missing; run bun run build:package');
}

if (failures.length) {
  console.error(failures.join('\n'));
  process.exit(1);
}
console.log('[check-pure-rust-exports] PASS pure-Rust library export contract');
