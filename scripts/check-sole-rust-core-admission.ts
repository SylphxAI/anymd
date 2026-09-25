#!/usr/bin/env bun
/**
 * Core sole-Rust candidate admission subset.
 * Not full showhand release authorization — only the currently required core gates.
 */
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { resolveCargoReleaseDir } from './native/cargo-target-dir.ts';

const root = join(import.meta.dirname, '..');
const steps = [
  ['bun', ['run', 'check:pure-rust-matrix']],
  ['bun', ['run', 'check:semantic-contracts']],
  ['bun', ['run', 'check:agent-task-corpus']],
  ['bun', ['run', 'check:ts-production-absence']],
  ['bun', ['run', 'check:prod-dependency-closure']],
  ['bun', ['run', 'check:pure-rust-exports']],
  ['bun', ['run', 'package:smoke']],
  ['bun', ['run', 'check:production-contract']],
  ['bun', ['test', 'test/production/capabilityParity.contract.test.ts', '--timeout=600000']],
  ['bun', ['run', 'test:ts-vs-rust-text']],
  ['bun', ['run', 'test:agent-task-eval']],
];

const results: Array<{ step: string; ok: boolean; code: number | null }> = [];
for (const [cmd, args] of steps) {
  const label = [cmd, ...args].join(' ');
  console.log(`[sole-rust-core] RUN ${label}`);
  const r = spawnSync(cmd, args, {
    cwd: root,
    stdio: 'inherit',
    env: {
      ...process.env,
      ANYMD_RUST_BIN:
        process.env.ANYMD_RUST_BIN ||
        join(resolveCargoReleaseDir(root), 'anymd'),
    },
  });
  const ok = r.status === 0;
  results.push({ step: label, ok, code: r.status });
  if (!ok) {
    console.error(`[sole-rust-core] FAIL ${label}`);
    console.error(JSON.stringify({ profile: 'sole_rust_core_admission', pass: false, results }, null, 2));
    process.exit(r.status ?? 1);
  }
}

console.log(
  JSON.stringify(
    {
      profile: 'sole_rust_core_admission',
      pass: true,
      results,
      note: 'Core candidate gates only. Not independent whole-product review, five-host proof, admissible A/B, or channel publish authorization.',
    },
    null,
    2
  )
);
console.log('[sole-rust-core] PASS');
