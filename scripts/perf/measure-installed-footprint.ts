#!/usr/bin/env bun
/**
 * Measure clean-install product footprint for a published package version.
 * Compares full install closure — never wrapper tarball vs native exe alone.
 */
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const version = (process.argv.find((a) => a.startsWith('--version=')) || '--version=4.0.2').split('=')[1]!;
const pkg = `@sylphx/anymd@${version}`;
const root = mkdtempSync(join(tmpdir(), `pdf-footprint-${version}-`));

function run(cmd: string, args: string[], cwd = root) {
  const r = spawnSync(cmd, args, { cwd, encoding: 'utf8' });
  if (r.status !== 0) {
    throw new Error(`${cmd} ${args.join(' ')} failed: ${r.stderr || r.stdout}`);
  }
  return r.stdout;
}

function duBytes(path: string): number {
  const r = spawnSync('du', ['-sb', path], { encoding: 'utf8' });
  if (r.status === 0) return Number((r.stdout || '').split(/\s+/)[0] || 0);
  // macOS du lacks -sb; fall back
  const r2 = spawnSync('du', ['-sk', path], { encoding: 'utf8' });
  const kb = Number((r2.stdout || '').split(/\s+/)[0] || 0);
  return kb * 1024;
}

function countFiles(dir: string): number {
  let n = 0;
  const walk = (p: string) => {
    for (const ent of readdirSync(p, { withFileTypes: true })) {
      const fp = join(p, ent.name);
      if (ent.isDirectory()) walk(fp);
      else n += 1;
    }
  };
  walk(dir);
  return n;
}

run('npm', ['init', '-y']);
run('npm', ['install', pkg, '--no-fund', '--no-audit']);
const nm = join(root, 'node_modules');
const main = join(nm, '@sylphx', 'pdf-reader-mcp');
const footprint = {
  package: pkg,
  host: {
    platform: process.platform,
    arch: process.arch,
    node: process.version,
  },
  installDir: root,
  mainPackageBytes: duBytes(main),
  nodeModulesBytes: duBytes(nm),
  nodeModulesFiles: countFiles(nm),
  productionDependencies: JSON.parse(
    run('node', ['-e', "console.log(JSON.stringify(require('./node_modules/@sylphx/pdf-reader-mcp/package.json').dependencies||{}))"])
  ),
  nativePackages: readdirSync(join(nm, '@sylphx')).filter((n) => n.startsWith('anymd-')),
};

const out = join(process.cwd(), 'benchmark-artifacts', 'installed-footprint', `${version}-${process.platform}-${process.arch}.json`);
spawnSync('mkdir', ['-p', join(process.cwd(), 'benchmark-artifacts', 'installed-footprint')]);
writeFileSync(out, JSON.stringify(footprint, null, 2) + '\n');
console.log(JSON.stringify(footprint, null, 2));
console.log(`wrote ${out}`);
