#!/usr/bin/env bun
/**
 * Pure-Rust MCP performance benchmark.
 *
 * Measures wall-clock latency for the production pure-Rust binary across fixed
 * scenarios. Optionally compares against the legacy TypeScript engine when
 * Residual TypeScript sources are differential oracles only and are never invoked.
 *
 * Usage:
 *   bun scripts/benchmark-pure-rust.ts
 *   bun scripts/benchmark-pure-rust.ts --iterations 30 --warmup 5
 *   bun scripts/benchmark-pure-rust.ts --output benchmark-artifacts/pdf_pure_rust_benchmark.json
 */

import { spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { resolveCargoReleaseDir } from './native/cargo-target-dir.ts';

const repoRoot = path.resolve(import.meta.dirname, '..');
const samplePdf = path.join(repoRoot, 'test/fixtures/sample.pdf');
const rustBinCandidates = [
  process.env.ANYMD_RUST_BIN,
  path.join(repoRoot, 'bin/native/anymd'),
  path.join(resolveCargoReleaseDir(repoRoot), 'anymd'),
].filter(Boolean) as string[];

const args = process.argv.slice(2);
const iterations = Number(args.find((_, i, a) => a[i - 1] === '--iterations') ?? 20);
const warmup = Number(args.find((_, i, a) => a[i - 1] === '--warmup') ?? 3);
const outputPath = args.find((_, i, a) => a[i - 1] === '--output');

type JsonRpc = {
  jsonrpc: '2.0';
  id?: number;
  method?: string;
  params?: unknown;
  result?: unknown;
  error?: { message?: string };
};

type Scenario = {
  id: string;
  tool: string;
  args: Record<string, unknown>;
};

const scenarios: Scenario[] = [
  {
    id: 'metadata_page_count',
    tool: 'read_pdf',
    args: {
      sources: [{ path: samplePdf }],
      auto: false,
      include_metadata: true,
      include_page_count: true,
    },
  },
  {
    id: 'full_text',
    tool: 'read_pdf',
    args: {
      sources: [{ path: samplePdf }],
      auto: false,
      include_full_text: true,
      include_page_count: true,
    },
  },
  {
    id: 'agent_document_twin_balanced',
    tool: 'read_pdf',
    args: {
      sources: [{ path: samplePdf }],
      auto: true,
      auto_detail: 'balanced',
    },
  },
  {
    id: 'agent_document_twin_full',
    tool: 'read_pdf',
    args: {
      sources: [{ path: samplePdf }],
      auto: false,
      include_full_text: true,
      include_markdown: true,
      include_chunks: true,
      include_elements: true,
      include_text_layer: true,
      include_tables: true,
      include_document_map: true,
      include_document_ast: true,
      include_safety_findings: true,
      include_layout_diagnostics: true,
      include_trust_report: true,
      include_accessibility_report: true,
    },
  },
  {
    id: 'search_literal',
    tool: 'search_pdf',
    args: {
      sources: [{ path: samplePdf }],
      query: 'a',
      max_matches_per_source: 20,
    },
  },
  {
    id: 'inspect',
    tool: 'pdf_evidence',
    args: {
      operation: 'inspect',
      sources: [{ path: samplePdf }],
      sample_pages: 3,
    },
  },
];

function resolveRustBin(): string {
  for (const candidate of rustBinCandidates) {
    if (candidate && fs.existsSync(candidate) && fs.statSync(candidate).mode & 0o111) {
      return candidate;
    }
  }
  throw new Error('Rust MCP binary not found. Run: bun run build:rust');
}

function spawnMcp(bin: string): ChildProcess {
  return spawn(bin, [], {
    cwd: repoRoot,
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, MCP_TRANSPORT: 'stdio', PDF_READER_MCP_TRANSPORT: 'stdio' },
  });
}

async function rpc(
  proc: ChildProcess,
  id: number,
  method: string,
  params?: unknown,
  timeoutMs = 60_000
): Promise<JsonRpc> {
  return new Promise((resolve, reject) => {
    let buffer = '';
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`timeout waiting for ${method}`));
    }, timeoutMs);

    const onData = (chunk: Buffer) => {
      buffer += chunk.toString('utf8');
      const lines = buffer.split('\n');
      buffer = lines.pop() ?? '';
      for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed) continue;
        try {
          const msg = JSON.parse(trimmed) as JsonRpc;
          if (msg.id === id) {
            cleanup();
            resolve(msg);
            return;
          }
        } catch {
          // ignore non-json
        }
      }
    };

    const cleanup = () => {
      clearTimeout(timer);
      proc.stdout?.off('data', onData);
    };

    proc.stdout?.on('data', onData);
    proc.stdin?.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
  });
}

async function initialize(proc: ChildProcess): Promise<void> {
  await rpc(proc, 1, 'initialize', {
    protocolVersion: '2024-11-05',
    capabilities: {},
    clientInfo: { name: 'pure-rust-benchmark', version: '1.0.0' },
  });
  proc.stdin?.write(
    `${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`
  );
}

function stats(samples: number[]) {
  const sorted = [...samples].sort((a, b) => a - b);
  const sum = samples.reduce((a, b) => a + b, 0);
  const avg = sum / samples.length;
  const p50 = sorted[Math.floor(sorted.length * 0.5)] ?? 0;
  const p95 = sorted[Math.floor(sorted.length * 0.95)] ?? sorted[sorted.length - 1] ?? 0;
  return {
    avg_ms: Number(avg.toFixed(3)),
    min_ms: Number((sorted[0] ?? 0).toFixed(3)),
    max_ms: Number((sorted[sorted.length - 1] ?? 0).toFixed(3)),
    p50_ms: Number(p50.toFixed(3)),
    p95_ms: Number(p95.toFixed(3)),
    samples: samples.length,
  };
}

async function measureScenario(proc: ChildProcess, scenario: Scenario, startId: number) {
  const samples: number[] = [];
  let id = startId;
  for (let i = 0; i < warmup; i += 1) {
    id += 1;
    const res = await rpc(proc, id, 'tools/call', {
      name: scenario.tool,
      arguments: scenario.args,
    });
    if (res.error) throw new Error(`${scenario.id} warmup failed: ${res.error.message}`);
  }
  for (let i = 0; i < iterations; i += 1) {
    id += 1;
    const t0 = performance.now();
    const res = await rpc(proc, id, 'tools/call', {
      name: scenario.tool,
      arguments: scenario.args,
    });
    const t1 = performance.now();
    if (res.error) throw new Error(`${scenario.id} failed: ${res.error.message}`);
    samples.push(t1 - t0);
  }
  return { id, stats: stats(samples) };
}

async function main() {
  if (!fs.existsSync(samplePdf)) {
    throw new Error(`Missing fixture: ${samplePdf}`);
  }
  const bin = resolveRustBin();
  const proc = spawnMcp(bin);
  const stderr: string[] = [];
  proc.stderr?.on('data', (c) => stderr.push(c.toString()));

  try {
    await initialize(proc);
    let nextId = 10;
    const results: Record<string, ReturnType<typeof stats>> = {};
    for (const scenario of scenarios) {
      const measured = await measureScenario(proc, scenario, nextId);
      nextId = measured.id + 1;
      results[scenario.id] = measured.stats;
      console.error(
        `${scenario.id}: avg=${measured.stats.avg_ms}ms p50=${measured.stats.p50_ms}ms p95=${measured.stats.p95_ms}ms`
      );
    }

    // Historical TS baseline from docs/benchmark.md (checked-in release evidence).
    const historicalTsBaseline = {
      source: 'docs/benchmark.md (pre pure-Rust release gate, sample.pdf)',
      metadata_page_count_avg_ms: 1.1,
      full_text_avg_ms: 16.1,
      agent_document_twin_avg_ms: 27.2,
    };

    const pure = results;
    const comparison = {
      metadata_page_count: {
        historical_ts_avg_ms: historicalTsBaseline.metadata_page_count_avg_ms,
        pure_rust_avg_ms: pure.metadata_page_count.avg_ms,
        speedup:
          historicalTsBaseline.metadata_page_count_avg_ms / Math.max(pure.metadata_page_count.avg_ms, 0.001),
      },
      full_text: {
        historical_ts_avg_ms: historicalTsBaseline.full_text_avg_ms,
        pure_rust_avg_ms: pure.full_text.avg_ms,
        speedup: historicalTsBaseline.full_text_avg_ms / Math.max(pure.full_text.avg_ms, 0.001),
      },
      agent_document_twin: {
        historical_ts_avg_ms: historicalTsBaseline.agent_document_twin_avg_ms,
        pure_rust_avg_ms: pure.agent_document_twin_balanced.avg_ms,
        speedup:
          historicalTsBaseline.agent_document_twin_avg_ms /
          Math.max(pure.agent_document_twin_balanced.avg_ms, 0.001),
      },
    };

    const artifact = {
      schemaVersion: 1,
      engine: 'pure-rust',
      binary: bin,
      fixture: samplePdf,
      iterations,
      warmup,
      measuredAt: new Date().toISOString(),
      host: {
        platform: process.platform,
        arch: process.arch,
        node: process.version,
      },
      scenarios: pure,
      historicalTsBaseline,
      comparison,
      notes: [
        'Historical TypeScript numbers come from the previous release-gate artifact on the same sample.pdf fixture.',
        'Pure-Rust numbers are measured against the production rmcp binary in this run.',
        'Absolute ms vary by host; speedup ratios are the primary comparison signal.',
      ],
    };

    const json = JSON.stringify(artifact, null, 2);
    console.log(json);
    if (outputPath) {
      fs.mkdirSync(path.dirname(path.resolve(outputPath)), { recursive: true });
      fs.writeFileSync(outputPath, `${json}\n`);
    }
  } finally {
    proc.kill('SIGTERM');
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
