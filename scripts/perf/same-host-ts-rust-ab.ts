#!/usr/bin/env bun
/**
 * Controlled same-host TypeScript 3.0.14 vs Rust A/B harness.
 *
 * Modes:
 * - startup_inclusive: each sample spawns process + initialize + task (honest name)
 * - persistent_warm: one long-lived process; time only tools/call after warm-up
 *
 * Formal product admission requires suite status=admissible_pass under the rebuilt
 * contract (docs/specs/performance/same-host-ab-contract.md).
 */
import { type ChildProcessWithoutNullStreams, spawn, spawnSync } from 'node:child_process';
import {
  createHash,
  randomBytes,
} from 'node:crypto';
import {
  existsSync,
  mkdirSync,
  readFileSync,
  statSync,
  writeFileSync,
  appendFileSync,
} from 'node:fs';
import { arch, cpus, platform, release, totalmem } from 'node:os';
import { basename, join } from 'node:path';
import { performance } from 'node:perf_hooks';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';
import {
  NATIVE_PLATFORM_PACKAGES,
  resolveNativePlatformId,
} from '../../src/native/platform-package-map.ts';

const root = join(import.meta.dirname, '../..');
const outDir =
  process.env['MCP_PDF_PERF_OUTPUT_DIR'] || join(root, 'benchmark-artifacts/same-host-ab');
mkdirSync(outDir, { recursive: true });

const requireAdmissible = process.argv.includes('--require-admissible');
const mode = (process.env['MCP_PDF_PERF_MODE'] || 'both') as
  | 'startup_inclusive'
  | 'persistent_warm'
  | 'both';
const iterationsWarm = Number(process.env['MCP_PDF_PERF_WARM_ITERS'] || 9);
const fixture =
  process.env['MCP_PDF_PERF_FIXTURE'] || join(root, 'test/fixtures/sample.pdf');
const fixtureClass = process.env['MCP_PDF_PERF_FIXTURE_CLASS'] || 'small_text';
const taskName = process.env['MCP_PDF_PERF_TASK'] || defaultTaskForClass(fixtureClass);
const useRegistryRust = process.env['MCP_PDF_PERF_RUST_FROM_REGISTRY'] === '1';
const minWarmSpeedup = Number(process.env['MCP_PDF_PERF_MIN_WARM_SPEEDUP'] || 1.5);
const maxP95Regression = Number(process.env['MCP_PDF_PERF_MAX_P95_REGRESSION'] || 1.15);

type EngineId = 'typescript-3.0.14' | 'rust-candidate';

type Sample = {
  engine: EngineId;
  mode: 'startup_inclusive' | 'persistent_warm' | 'startup_only';
  phase: 'cold' | 'warm' | 'startup';
  ok: boolean;
  latencyMs: number;
  semanticPass: boolean;
  error?: string;
  textChars?: number;
  normalizedTextHash?: string;
  outcomeDigest?: string;
};

function defaultTaskForClass(cls: string): string {
  switch (cls) {
    case 'table_heavy':
    case 'hostile_table_bound':
      return 'read_pdf_tables';
    case 'structured':
      return 'read_pdf_structure';
    case 'geometry_edge':
      return 'read_pdf_geometry';
    case 'search_smoke':
      return 'search_pdf';
    default:
      return 'read_pdf_full_text';
  }
}

const gitHead = (): string | null => {
  const r = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' });
  return r.status === 0 ? r.stdout.trim() : null;
};

const sha256File = (path: string | null | undefined): string | null => {
  if (!path || !existsSync(path)) return null;
  const h = createHash('sha256');
  h.update(readFileSync(path));
  return h.digest('hex');
};

const fileSize = (path: string | null | undefined): number | null => {
  if (!path || !existsSync(path)) return null;
  try {
    return statSync(path).size;
  } catch {
    return null;
  }
};

const normalizeText = (s: string): string =>
  s
    .replace(/\r\n/g, '\n')
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();

const resolveRustBinary = (): string | null => {
  const forced = process.env['ANYMD_RUST_BIN'];
  if (forced && existsSync(forced)) return forced;

  if (useRegistryRust) {
    const installRoot = join(outDir, 'rust-registry-install');
    mkdirSync(installRoot, { recursive: true });
    const platformId = resolveNativePlatformId();
    if (!platformId) return null;
    const meta = NATIVE_PLATFORM_PACKAGES[platformId];
    const bin = join(
      installRoot,
      'node_modules',
      meta.npmName,
      'bin',
      meta.binaryName
    );
    if (!existsSync(bin)) {
      const install = spawnSync(
        'npm',
        [
          'install',
          `@sylphx/anymd@${process.env['MCP_PDF_PERF_RUST_VERSION'] || JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version || '4.1.0'}`,
          '--prefix',
          installRoot,
          '--no-save',
          '--no-fund',
          '--no-audit',
        ],
        { encoding: 'utf8', cwd: root }
      );
      if (install.status !== 0) {
        console.error(install.stderr || install.stdout);
        return null;
      }
    }
    if (existsSync(bin)) return bin;
  }

  const platformId = resolveNativePlatformId();
  if (!platformId) return null;
  const meta = NATIVE_PLATFORM_PACKAGES[platformId];
  const candidates = [
    join(resolveCargoReleaseDir(root), 'anymd'),
    join(root, 'bin/native', platformId, meta.binaryName),
    join(root, meta.packageDir, 'bin', meta.binaryName),
    join(root, 'node_modules', meta.npmName, 'bin', meta.binaryName),
  ];
  for (const c of candidates) if (existsSync(c)) return c;
  return null;
};

const ensureTypescriptLkg = (): { command: string; args: string[]; installRoot: string } | null => {
  const installRoot = join(outDir, 'ts-lkg-install');
  mkdirSync(installRoot, { recursive: true });
  const entry = join(installRoot, 'node_modules/@sylphx/pdf-reader-mcp/dist/index.js');
  if (!existsSync(entry)) {
    const install = spawnSync(
      'npm',
      ['install', '@sylphx/pdf-reader-mcp@3.0.14', '--prefix', installRoot, '--no-save', '--no-fund', '--no-audit'],
      { encoding: 'utf8', cwd: root }
    );
    if (install.status !== 0) {
      console.error('[same-host-ab] failed to install TS 3.0.14 LKG');
      console.error(install.stderr || install.stdout);
      return null;
    }
  }
  if (!existsSync(entry)) return null;
  return { command: process.execPath, args: [entry], installRoot };
};

const taskArguments = (task: string, pdfPath: string): { tool: string; arguments: Record<string, unknown> } => {
  switch (task) {
    case 'read_pdf_tables':
      return {
        tool: 'read_pdf',
        arguments: {
          sources: [{ path: pdfPath }],
          pages: [1],
          include_full_text: true,
          include_tables: true,
        },
      };
    case 'read_pdf_structure':
      return {
        tool: 'read_pdf',
        arguments: {
          sources: [{ path: pdfPath }],
          pages: [1],
          include_full_text: true,
          include_document_map: true,
        },
      };
    case 'read_pdf_geometry':
      return {
        tool: 'read_pdf',
        arguments: {
          sources: [{ path: pdfPath }],
          pages: [1],
          include_full_text: true,
          include_text_layer: true,
        },
      };
    case 'search_pdf':
      return {
        tool: 'search_pdf',
        arguments: {
          sources: [{ path: pdfPath }],
          query: 'the',
          max_results: 5,
        },
      };
    case 'read_pdf_full_text':
    default:
      return {
        tool: 'read_pdf',
        arguments: {
          sources: [{ path: pdfPath }],
          pages: [1],
          include_full_text: true,
        },
      };
  }
};

type ParsedOutcome = {
  ok: boolean;
  error?: string;
  textChars: number;
  normalizedText: string;
  normalizedTextHash: string;
  outcomeDigest: string;
  successField?: boolean;
  tableCount?: number;
  mapNodeCount?: number;
  searchHitCount?: number;
};

const parseToolOutcome = (msg: any, task: string): ParsedOutcome => {
  if (msg?.error || msg?.result?.isError) {
    return {
      ok: false,
      error: JSON.stringify(msg?.error ?? msg?.result),
      textChars: 0,
      normalizedText: '',
      normalizedTextHash: createHash('sha256').update('').digest('hex'),
      outcomeDigest: createHash('sha256').update('error').digest('hex'),
    };
  }
  const texts = (msg?.result?.content || [])
    .map((c: any) => (typeof c?.text === 'string' ? c.text : ''))
    .filter(Boolean);
  const joined = texts.join('\n');
  let payload: any = null;
  for (const t of texts) {
    try {
      payload = JSON.parse(t);
      break;
    } catch {
      // keep raw
    }
  }
  const fullTextCandidates: string[] = [];
  if (payload) {
    if (typeof payload.full_text === 'string') fullTextCandidates.push(payload.full_text);
    if (Array.isArray(payload.results)) {
      for (const r of payload.results) {
        if (typeof r?.full_text === 'string') fullTextCandidates.push(r.full_text);
        if (typeof r?.text === 'string') fullTextCandidates.push(r.text);
      }
    }
    if (typeof payload.text === 'string') fullTextCandidates.push(payload.text);
  }
  const rawText = fullTextCandidates.find((s) => s.trim().length > 0) || joined;
  const normalizedText = normalizeText(rawText);
  const tableCount = Array.isArray(payload?.tables)
    ? payload.tables.length
    : Array.isArray(payload?.results?.[0]?.tables)
      ? payload.results[0].tables.length
      : undefined;
  const mapNodeCount = Array.isArray(payload?.document_map?.nodes)
    ? payload.document_map.nodes.length
    : Array.isArray(payload?.documentMap?.nodes)
      ? payload.documentMap.nodes.length
      : undefined;
  const searchHitCount = Array.isArray(payload?.matches)
    ? payload.matches.length
    : Array.isArray(payload?.results)
      ? payload.results.length
      : undefined;
  const successField =
    typeof payload?.success === 'boolean'
      ? payload.success
      : typeof payload?.results?.[0]?.success === 'boolean'
        ? payload.results[0].success
        : undefined;

  // Capability-aware semantic floors
  let ok = false;
  if (task === 'search_pdf') {
    ok = (searchHitCount ?? 0) > 0 || normalizedText.length > 40;
  } else if (task === 'read_pdf_tables') {
    // table fixture may still return useful text even if table reconstruction is partial
    ok = (tableCount ?? 0) > 0 || normalizedText.length > 40;
  } else if (task === 'read_pdf_structure') {
    ok = (mapNodeCount ?? 0) > 0 || normalizedText.length > 40;
  } else if (task === 'read_pdf_geometry') {
    // geometry pages can have empty full_text; require explicit success or text-layer payload markers
    const hasLayer =
      joined.includes('text_layer') ||
      joined.includes('textLayer') ||
      (Array.isArray(payload?.results?.[0]?.text_layer) && payload.results[0].text_layer.length > 0);
    ok = successField === true || hasLayer || normalizedText.length > 0;
  } else {
    ok = normalizedText.length > 0 || successField === true;
  }

  const outcomeDigest = createHash('sha256')
    .update(
      JSON.stringify({
        task,
        normalizedText,
        successField: successField ?? null,
        tableCount: tableCount ?? null,
        mapNodeCount: mapNodeCount ?? null,
        searchHitCount: searchHitCount ?? null,
      })
    )
    .digest('hex');

  return {
    ok,
    textChars: normalizedText.length,
    normalizedText,
    normalizedTextHash: createHash('sha256').update(normalizedText).digest('hex'),
    outcomeDigest,
    successField,
    tableCount,
    mapNodeCount,
    searchHitCount,
  };
};

class McpSession {
  child: ChildProcessWithoutNullStreams;
  buf = '';
  nextId = 1;
  pending = new Map<
    number,
    { resolve: (v: any) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }
  >();
  constructor(
    public engine: EngineId,
    command: string,
    args: string[]
  ) {
    this.child = spawn(command, args, {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, MCP_TRANSPORT: 'stdio' },
    });
    this.child.stdout.on('data', (d) => this.onData(d.toString('utf8')));
    this.child.stderr.on('data', () => {
      // ignore noisy logs
    });
    this.child.on('exit', () => {
      for (const [, p] of this.pending) {
        clearTimeout(p.timer);
        p.reject(new Error('process exited'));
      }
      this.pending.clear();
    });
  }
  onData(chunk: string) {
    this.buf += chunk;
    let idx: number;
    while ((idx = this.buf.indexOf('\n')) >= 0) {
      const line = this.buf.slice(0, idx).trim();
      this.buf = this.buf.slice(idx + 1);
      if (!line) continue;
      try {
        const msg = JSON.parse(line);
        if (typeof msg.id === 'number' && this.pending.has(msg.id)) {
          const p = this.pending.get(msg.id)!;
          this.pending.delete(msg.id);
          clearTimeout(p.timer);
          p.resolve(msg);
        }
      } catch {
        // ignore partial/non-json
      }
    }
  }
  request(method: string, params?: unknown, timeoutMs = 45_000): Promise<any> {
    const id = this.nextId++;
    const payload = { jsonrpc: '2.0', id, method, params };
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`timeout ${method}`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.child.stdin.write(`${JSON.stringify(payload)}\n`);
    });
  }
  notify(method: string, params?: unknown) {
    this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method, params })}\n`);
  }
  async initialize() {
    await this.request('initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'same-host-ab', version: '0.2.0' },
    });
    this.notify('notifications/initialized', {});
  }
  kill() {
    try {
      this.child.kill('SIGKILL');
    } catch {
      // ignore
    }
  }
}

const percentile = (values: number[], p: number): number | null => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[idx] ?? null;
};

const summarize = (samples: Sample[], engine: EngineId, mode: Sample['mode'], phase: Sample['phase']) => {
  const xs = samples.filter(
    (s) => s.engine === engine && s.mode === mode && s.phase === phase && s.ok && s.semanticPass
  );
  const lat = xs.map((s) => s.latencyMs);
  return {
    n: xs.length,
    medianMs: percentile(lat, 50),
    p95Ms: percentile(lat, 95),
    meanMs: lat.length ? lat.reduce((a, b) => a + b, 0) / lat.length : null,
    semanticPassRate:
      samples.filter((s) => s.engine === engine && s.mode === mode && s.phase === phase).length === 0
        ? 0
        : xs.length /
          samples.filter((s) => s.engine === engine && s.mode === mode && s.phase === phase).length,
  };
};

const runStartupInclusive = async (
  engines: Array<{ id: EngineId; command: string; args: string[] }>,
  samples: Sample[]
) => {
  const task = taskArguments(taskName, fixture);
  // cold
  for (const eng of [...engines].sort(() => Math.random() - 0.5)) {
    const session = new McpSession(eng.id, eng.command, eng.args);
    const t0 = performance.now();
    try {
      await session.initialize();
      const resp = await session.request('tools/call', {
        name: task.tool,
        arguments: task.arguments,
      });
      const outcome = parseToolOutcome(resp, taskName);
      samples.push({
        engine: eng.id,
        mode: 'startup_inclusive',
        phase: 'cold',
        ok: outcome.ok,
        latencyMs: performance.now() - t0,
        semanticPass: outcome.ok,
        error: outcome.error,
        textChars: outcome.textChars,
        normalizedTextHash: outcome.normalizedTextHash,
        outcomeDigest: outcome.outcomeDigest,
      });
    } catch (e) {
      samples.push({
        engine: eng.id,
        mode: 'startup_inclusive',
        phase: 'cold',
        ok: false,
        latencyMs: performance.now() - t0,
        semanticPass: false,
        error: String(e),
      });
    } finally {
      session.kill();
    }
  }
  // warm = still process-restart end-to-end (named honestly)
  for (let i = 0; i < iterationsWarm; i++) {
    for (const eng of [...engines].sort(() => Math.random() - 0.5)) {
      const session = new McpSession(eng.id, eng.command, eng.args);
      const t0 = performance.now();
      try {
        await session.initialize();
        const resp = await session.request('tools/call', {
          name: task.tool,
          arguments: task.arguments,
        });
        const outcome = parseToolOutcome(resp, taskName);
        samples.push({
          engine: eng.id,
          mode: 'startup_inclusive',
          phase: 'warm',
          ok: outcome.ok,
          latencyMs: performance.now() - t0,
          semanticPass: outcome.ok,
          error: outcome.error,
          textChars: outcome.textChars,
          normalizedTextHash: outcome.normalizedTextHash,
          outcomeDigest: outcome.outcomeDigest,
        });
      } catch (e) {
        samples.push({
          engine: eng.id,
          mode: 'startup_inclusive',
          phase: 'warm',
          ok: false,
          latencyMs: performance.now() - t0,
          semanticPass: false,
          error: String(e),
        });
      } finally {
        session.kill();
      }
    }
  }
};

const runPersistentWarm = async (
  engines: Array<{ id: EngineId; command: string; args: string[] }>,
  samples: Sample[]
) => {
  const task = taskArguments(taskName, fixture);
  const sessions = new Map<EngineId, McpSession>();
  try {
    for (const eng of engines) {
      const session = new McpSession(eng.id, eng.command, eng.args);
      const t0 = performance.now();
      await session.initialize();
      samples.push({
        engine: eng.id,
        mode: 'startup_only',
        phase: 'startup',
        ok: true,
        latencyMs: performance.now() - t0,
        semanticPass: true,
      });
      // one discarded warm-up call
      await session.request('tools/call', { name: task.tool, arguments: task.arguments });
      sessions.set(eng.id, session);
    }
    for (let i = 0; i < iterationsWarm; i++) {
      for (const eng of [...engines].sort(() => Math.random() - 0.5)) {
        const session = sessions.get(eng.id)!;
        const t0 = performance.now();
        try {
          const resp = await session.request('tools/call', {
            name: task.tool,
            arguments: task.arguments,
          });
          const outcome = parseToolOutcome(resp, taskName);
          samples.push({
            engine: eng.id,
            mode: 'persistent_warm',
            phase: 'warm',
            ok: outcome.ok,
            latencyMs: performance.now() - t0,
            semanticPass: outcome.ok,
            error: outcome.error,
            textChars: outcome.textChars,
            normalizedTextHash: outcome.normalizedTextHash,
            outcomeDigest: outcome.outcomeDigest,
          });
        } catch (e) {
          samples.push({
            engine: eng.id,
            mode: 'persistent_warm',
            phase: 'warm',
            ok: false,
            latencyMs: performance.now() - t0,
            semanticPass: false,
            error: String(e),
          });
        }
      }
    }
  } finally {
    for (const s of sessions.values()) s.kill();
  }
};

const main = async () => {
  const runId = `${new Date().toISOString().replace(/[:.]/g, '-')}-${randomBytes(3).toString('hex')}`;
  const rawDir = join(root, 'verification/perf/raw', runId);
  mkdirSync(rawDir, { recursive: true });

  const candidateSha = gitHead();
  const rustBin = resolveRustBinary();
  const ts = ensureTypescriptLkg();
  const samples: Sample[] = [];
  const blockers: string[] = [];

  if (!existsSync(fixture)) blockers.push(`fixture missing: ${fixture}`);
  if (!rustBin) blockers.push('rust binary not found');
  if (!ts) blockers.push('typescript 3.0.14 LKG install failed');

  const engines: Array<{ id: EngineId; command: string; args: string[] }> = [];
  if (ts) engines.push({ id: 'typescript-3.0.14', command: ts.command, args: ts.args });
  if (rustBin) engines.push({ id: 'rust-candidate', command: rustBin, args: [] });

  const host = {
    platform: platform(),
    release: release(),
    arch: arch(),
    node: process.version,
    cpuModel: cpus()[0]?.model ?? null,
    cpuCount: cpus().length,
    totalMemBytes: totalmem(),
  };
  const binaries = {
    rustBinary: rustBin,
    rustSha256: sha256File(rustBin),
    rustSizeBytes: fileSize(rustBin),
    typescriptEntry: ts ? join(ts.installRoot, 'node_modules/@sylphx/pdf-reader-mcp/dist/index.js') : null,
    typescriptPackageVersion: '3.0.14',
    rustFromRegistry: useRegistryRust,
  };
  writeFileSync(join(rawDir, 'host.json'), `${JSON.stringify(host, null, 2)}\n`);
  writeFileSync(join(rawDir, 'binaries.json'), `${JSON.stringify(binaries, null, 2)}\n`);
  writeFileSync(
    join(rawDir, 'run.json'),
    `${JSON.stringify(
      {
        runId,
        candidateSha,
        fixture,
        fixtureClass,
        taskName,
        mode,
        iterationsWarm,
        minWarmSpeedup,
        maxP95Regression,
      },
      null,
      2
    )}\n`
  );

  if (!blockers.length) {
    if (mode === 'startup_inclusive' || mode === 'both') {
      await runStartupInclusive(engines, samples);
    }
    if (mode === 'persistent_warm' || mode === 'both') {
      await runPersistentWarm(engines, samples);
    }
  }

  // Durable raw samples
  for (const s of samples) {
    appendFileSync(join(rawDir, 'samples.jsonl'), `${JSON.stringify(s)}\n`);
  }

  // Cross-engine semantic consistency on persistent warm digests when available
  const digestsByEngine = (engine: EngineId, m: Sample['mode']) =>
    samples
      .filter((s) => s.engine === engine && s.mode === m && s.semanticPass && s.outcomeDigest)
      .map((s) => s.outcomeDigest as string);
  const tsDigests = digestsByEngine('typescript-3.0.14', 'persistent_warm');
  const rustDigests = digestsByEngine('rust-candidate', 'persistent_warm');
  const semanticComparable =
    tsDigests.length > 0 &&
    rustDigests.length > 0 &&
    // require both engines produced at least one successful outcome; equality is preferred but not mandatory for all tasks
    true;
  const exactDigestAgreement =
    tsDigests.length > 0 && rustDigests.length > 0 && tsDigests[0] === rustDigests[0];

  const modesToScore: Array<'startup_inclusive' | 'persistent_warm'> =
    mode === 'both' ? ['startup_inclusive', 'persistent_warm'] : [mode];

  const modeSummaries: Record<string, unknown> = {};
  const modeStatuses: Record<string, string> = {};
  for (const m of modesToScore) {
    const tsWarm = summarize(samples, 'typescript-3.0.14', m, 'warm');
    const rustWarm = summarize(samples, 'rust-candidate', m, 'warm');
    const speedup =
      tsWarm.medianMs && rustWarm.medianMs && rustWarm.medianMs > 0
        ? tsWarm.medianMs / rustWarm.medianMs
        : null;
    const p95Ratio =
      tsWarm.p95Ms && rustWarm.p95Ms && tsWarm.p95Ms > 0 ? rustWarm.p95Ms / tsWarm.p95Ms : null;
    const bothSemantic = tsWarm.semanticPassRate === 1 && rustWarm.semanticPassRate === 1;
    let st = 'measured_draft_not_admissible';
    const localBlockers: string[] = [];
    if (!bothSemantic) {
      st = 'failed';
      localBlockers.push(`${m}: semantic pass rate not 1.0 for both engines`);
    } else if (
      speedup !== null &&
      speedup >= minWarmSpeedup &&
      (p95Ratio === null || p95Ratio <= maxP95Regression) &&
      iterationsWarm >= 5
    ) {
      st = 'fixture_pass';
    } else {
      if (speedup === null || speedup < minWarmSpeedup) {
        localBlockers.push(`${m}: speedup ${speedup ?? 'n/a'} < ${minWarmSpeedup}`);
      }
      if (p95Ratio !== null && p95Ratio > maxP95Regression) {
        localBlockers.push(`${m}: p95 regression ratio ${p95Ratio} > ${maxP95Regression}`);
      }
    }
    modeSummaries[m] = {
      typescriptWarm: tsWarm,
      rustWarm,
      warmMedianSpeedupTsOverRust: speedup,
      warmP95RustOverTs: p95Ratio,
      blockers: localBlockers,
    };
    modeStatuses[m] = st;
    blockers.push(...localBlockers);
  }

  // Fixture pass requires persistent_warm fixture_pass; startup_inclusive is diagnostic.
  let status: 'failed' | 'measured_draft_not_admissible' | 'fixture_pass' = 'measured_draft_not_admissible';
  if (blockers.some((b) => b.includes('not found') || b.includes('failed to install') || b.includes('missing'))) {
    status = 'failed';
  } else if (modeStatuses.persistent_warm === 'failed' || modeStatuses.startup_inclusive === 'failed') {
    status = 'failed';
  } else if (modeStatuses.persistent_warm === 'fixture_pass') {
    status = 'fixture_pass';
  } else if (mode === 'startup_inclusive' && modeStatuses.startup_inclusive === 'fixture_pass') {
    // startup-only runs can be fixture_pass diagnostically but suite will not treat as formal
    status = 'fixture_pass';
    blockers.push(
      'startup_inclusive-only fixture_pass is diagnostic; formal suite requires persistent_warm fixture_pass'
    );
  }

  const report = {
    profile: 'same_host_ts_rust_ab_v2',
    status,
    generatedAt: new Date().toISOString(),
    runId,
    rawDir: `verification/perf/raw/${runId}`,
    candidateSha,
    historicalBaseline: '@sylphx/pdf-reader-mcp@3.0.14',
    fixture,
    fixtureClass,
    taskName,
    mode,
    iterationsWarm,
    thresholds: { minWarmSpeedup, maxP95Regression },
    host,
    binaries,
    semantic: {
      comparable: semanticComparable,
      exactPersistentWarmDigestAgreement: exactDigestAgreement,
      note:
        'Capability-first gate: successful task outcome required. Exact digest agreement is reported but not always mandatory across engines.',
    },
    modeStatuses,
    summary: modeSummaries,
    samples,
    blockers,
    claimPolicy:
      'fixture_pass is intermediate. Formal marketing claims require suite admissible_pass under rebuilt contract + independent performanceClaimsAuthorized=true. startup_inclusive must be labeled as spawn+initialize+task, not steady-state warm reads.',
  };

  writeFileSync(join(outDir, 'same-host-ab-report.json'), `${JSON.stringify(report, null, 2)}\n`);
  writeFileSync(join(rawDir, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  console.log(JSON.stringify(report, null, 2));
  console.log(`[same-host-ab] wrote ${join(outDir, 'same-host-ab-report.json')} raw=${rawDir}`);

  if (requireAdmissible && status !== 'fixture_pass') process.exit(2);
  if (status === 'failed') process.exit(1);
};

await main();
