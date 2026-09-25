#!/usr/bin/env bun
/**
 * Capability-first agent-task evaluation (ADR-0005).
 *
 * Measures semantic metrics on the same local corpus for TS baseline and
 * pure-Rust candidate. Thresholds come from measured TS baseline metrics.
 */
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { resolveCargoReleaseDir } from './native/cargo-target-dir.ts';
import {
  callMcpTool,
  evaluateAcceptance,
  extractMetrics,
  loadTasks,
  materializePublicTaskInput,
  publicDownloadsEnabled,
  publicPayload,
  publicTasksEnabled,
  resolveTaskEnv,
  selectTaskFiles,
  type Metrics,
  type RuntimeId,
  type Task,
  type TaskMeasurement,
} from './agent-task-shared.ts';

const root = join(import.meta.dirname, '..');
const corpusDir = join(root, 'docs/specs/agent-task-corpus');
const manifestPath = join(corpusDir, 'manifest.json');
const includePublic = publicTasksEnabled();
const allowDownloads = publicDownloadsEnabled();
const baselinePath = join(
  corpusDir,
  includePublic
    ? 'baselines/typescript-v3.0.14.public-url.json'
    : 'baselines/typescript-v3.0.14.local.json'
);
const rustServerPath = join(resolveCargoReleaseDir(root), 'anymd');
const tsServerPath = join(root, 'dist/index.js');

const args = process.argv.slice(2);
const writeBaseline = args.includes('--write-baseline');
const compareToBaseline = args.includes('--compare-baseline') || !args.includes('--no-compare');
const runtimes: RuntimeId[] = args.includes('--runtime=typescript')
  ? ['typescript']
  : args.includes('--runtime=pure-rust')
    ? ['pure-rust']
    : args.includes('--runtime=both') || writeBaseline
      ? ['typescript', 'pure-rust']
      : ['pure-rust'];

const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as {
  taskFiles: string[];
  publicTaskFiles?: string[];
  calibration?: { inventedNumericThresholdsForbidden?: boolean };
};

if (manifest.calibration?.inventedNumericThresholdsForbidden !== true) {
  console.error('[agent-task-eval] invented numeric thresholds are forbidden by manifest');
  process.exit(1);
}

const ensureRustServer = () => {
  if (existsSync(rustServerPath)) return;
  const build = spawnSync('cargo', ['build', '-p', 'pdf-reader-mcp-server', '--release'], {
    cwd: root,
    stdio: 'inherit',
  });
  if (build.status !== 0) {
    console.error('[agent-task-eval] pure-rust release server build failed');
    process.exit(1);
  }
};

const ensureTsServer = () => {
  if (existsSync(tsServerPath)) return;
  console.error('[agent-task-eval] missing dist/index.js; run bun run build first for TS baseline');
  process.exit(1);
};

const measureRuntime = async (runtime: RuntimeId, tasks: Task[]): Promise<TaskMeasurement[]> => {
  const out: TaskMeasurement[] = [];
  for (const task of tasks) {
    try {
      const baseEnv =
        runtime === 'pure-rust'
          ? {
              ...process.env,
              MCP_TRANSPORT: 'stdio',
              PDF_READER_ENGINE_MODE: 'pure-rust',
            }
          : {
              ...process.env,
              MCP_TRANSPORT: 'stdio',
              PDF_READER_ENGINE_MODE: '',
            };
      const env = resolveTaskEnv(task, root, baseEnv);
      const toolArgs = await materializePublicTaskInput(task, root, {
        allowDownloads,
      });
      const response =
        runtime === 'pure-rust'
          ? await callMcpTool({
              command: rustServerPath,
              env,
              tool: task.tool,
              toolArgs,
              cwd: root,
              timeoutMs: 120_000,
            })
          : await callMcpTool({
              command: process.execPath,
              args: [tsServerPath],
              env,
              tool: task.tool,
              toolArgs,
              cwd: root,
              timeoutMs: 120_000,
            });
      const payload = publicPayload(response);
      const metrics = extractMetrics(response, payload);
      const acceptance = evaluateAcceptance(task.acceptance, metrics);
      out.push({
        taskId: task.id,
        class: task.class,
        runtime,
        metrics,
        acceptancePass: acceptance.pass,
        acceptanceFailures: acceptance.failures,
      });
    } catch (error) {
      out.push({
        taskId: task.id,
        class: task.class,
        runtime,
        metrics: {
          success: false,
          isError: true,
          fullTextChars: 0,
          pageCount: null,
          tableCount: 0,
          matchCount: 0,
          firstMatchPage: null,
          firstTablePage: null,
          outlineCount: 0,
          formFieldCount: 0,
          annotationCount: 0,
          warningCount: 0,
          inventedFullTextRisk: false,
          visualCandidateCount: 0,
          visualEnrichmentCount: 0,
          ocrTextChars: 0,
          hasDocumentMap: false,
          containsTextHits: 0,
          fullTextPreview: '',
        },
        acceptancePass: false,
        acceptanceFailures: [error instanceof Error ? error.message : String(error)],
      });
    }
  }
  return out;
};

const compareMetricFloor = (
  taskId: string,
  metric: keyof Metrics,
  baseline: number,
  candidate: number
): string | null => {
  if (candidate + 1e-9 < baseline) {
    return `${taskId}: ${metric} below TS baseline (${candidate} < ${baseline})`;
  }
  return null;
};

const taskFiles = selectTaskFiles(manifest, includePublic);
if (includePublic && !(manifest.publicTaskFiles?.length)) {
  console.error('[agent-task-eval] public tasks requested but manifest.publicTaskFiles is empty');
  process.exit(1);
}
const tasks = loadTasks(root, taskFiles);
console.log(
  `[agent-task-eval] scope=${includePublic ? 'local+public-url' : 'local'} tasks=${tasks.length} downloads=${allowDownloads}`
);
if (runtimes.includes('pure-rust')) ensureRustServer();
if (runtimes.includes('typescript')) ensureTsServer();

const measurements: TaskMeasurement[] = [];
for (const runtime of runtimes) {
  measurements.push(...(await measureRuntime(runtime, tasks)));
}

const failures: string[] = [];
for (const item of measurements) {
  if (!item.acceptancePass) {
    failures.push(
      `${item.runtime}/${item.taskId}: acceptance failed: ${item.acceptanceFailures.join('; ')}`
    );
  }
}

if (writeBaseline) {
  const ts = measurements.filter((item) => item.runtime === 'typescript');
  if (ts.length !== tasks.length) {
    console.error('[agent-task-eval] cannot write baseline without complete typescript run');
    process.exit(1);
  }
  if (ts.some((item) => !item.acceptancePass)) {
    console.error('[agent-task-eval] refusing to write baseline with failing TS acceptance');
    for (const item of ts.filter((entry) => !entry.acceptancePass)) {
      console.error(`${item.taskId}: ${item.acceptanceFailures.join('; ')}`);
    }
    process.exit(1);
  }
  const baseline = {
    schemaVersion: 1,
    authority: 'ADR-0005',
    runtime: 'typescript-v3.0.14',
    measuredAt: new Date().toISOString(),
    method: 'scripts/run-agent-task-eval.ts --write-baseline',
    note: 'Measured local-fixture metrics including OCR/visual task classes. No invented % thresholds.',
    tasks: Object.fromEntries(
      ts.map((item) => [
        item.taskId,
        {
          class: item.class ?? null,
          metrics: item.metrics,
          acceptancePass: item.acceptancePass,
        },
      ])
    ),
  };
  mkdirSync(dirname(baselinePath), { recursive: true });
  writeFileSync(baselinePath, `${JSON.stringify(baseline, null, 2)}\n`);
  console.log(`[agent-task-eval] wrote measured TS baseline: ${baselinePath}`);
}

if (compareToBaseline && existsSync(baselinePath)) {
  const baseline = JSON.parse(readFileSync(baselinePath, 'utf8')) as {
    tasks: Record<string, { metrics: Metrics }>;
  };
  const rust = measurements.filter((item) => item.runtime === 'pure-rust');
  for (const item of rust) {
    const base = baseline.tasks[item.taskId]?.metrics;
    if (!base) {
      failures.push(`${item.taskId}: missing measured TS baseline entry`);
      continue;
    }
    const task = tasks.find((entry) => entry.id === item.taskId);
    const acceptance = task?.acceptance ?? {};
    const presence: Array<[keyof Metrics, number, number]> = [];
    if (typeof acceptance.minFullTextChars === 'number' || task?.class === 'extract_passage') {
      presence.push(['fullTextChars', base.fullTextChars > 0 ? 1 : 0, item.metrics.fullTextChars]);
    }
    if (typeof acceptance.minTables === 'number' || base.tableCount > 0) {
      presence.push(['tableCount', base.tableCount > 0 ? 1 : 0, item.metrics.tableCount]);
    }
    if (typeof acceptance.minMatches === 'number' || base.matchCount > 0) {
      presence.push(['matchCount', base.matchCount > 0 ? 1 : 0, item.metrics.matchCount]);
    }
    if (typeof acceptance.minOutlineItems === 'number' || base.outlineCount > 0) {
      presence.push(['outlineCount', base.outlineCount > 0 ? 1 : 0, item.metrics.outlineCount]);
    }
    if (typeof acceptance.minFormFields === 'number' || base.formFieldCount > 0) {
      presence.push([
        'formFieldCount',
        base.formFieldCount > 0 ? 1 : 0,
        item.metrics.formFieldCount,
      ]);
    }
    if (typeof acceptance.minAnnotations === 'number' || base.annotationCount > 0) {
      presence.push([
        'annotationCount',
        base.annotationCount > 0 ? 1 : 0,
        item.metrics.annotationCount,
      ]);
    }
    if (typeof acceptance.minVisualCandidates === 'number' || base.visualCandidateCount > 0) {
      presence.push([
        'visualCandidateCount',
        base.visualCandidateCount > 0 ? 1 : 0,
        item.metrics.visualCandidateCount,
      ]);
    }
    if (typeof acceptance.minVisualEnrichments === 'number' || base.visualEnrichmentCount > 0) {
      presence.push([
        'visualEnrichmentCount',
        base.visualEnrichmentCount > 0 ? 1 : 0,
        item.metrics.visualEnrichmentCount,
      ]);
    }
    if (
      acceptance.requireOcrTextLayer === true ||
      typeof acceptance.minOcrTextChars === 'number' ||
      base.ocrTextChars > 0
    ) {
      presence.push(['ocrTextChars', base.ocrTextChars > 0 ? 1 : 0, item.metrics.ocrTextChars]);
    }
    for (const [metric, required, c] of presence) {
      if (required <= 0) continue;
      const msg = compareMetricFloor(item.taskId, metric, required, c);
      if (msg) failures.push(`${msg} [TS measured presence]`);
    }
    if (acceptance.requireDocumentMap === true && base.hasDocumentMap && !item.metrics.hasDocumentMap) {
      failures.push(`${item.taskId}: document_map missing while TS baseline had document_map`);
    }
    if (acceptance.forbidInventedFullText === true) {
      if (base.inventedFullTextRisk === false && item.metrics.fullTextChars > 200) {
        failures.push(`${item.taskId}: invented full text risk above TS baseline`);
      }
    }
  }
} else if (compareToBaseline && !existsSync(baselinePath) && !writeBaseline) {
  failures.push(`missing measured baseline at ${baselinePath}; run with --write-baseline first`);
}

const summary = {
  runtimes,
  taskCount: tasks.length,
  measurements: measurements.map((item) => ({
    taskId: item.taskId,
    runtime: item.runtime,
    acceptancePass: item.acceptancePass,
    metrics: item.metrics,
  })),
  failureCount: failures.length,
};

if (failures.length) {
  console.error(failures.join('\n'));
  console.error(`[agent-task-eval] FAIL ${JSON.stringify(summary)}`);
  process.exit(1);
}

console.log(
  `[agent-task-eval] PASS ${tasks.length} tasks x ${runtimes.join(',')} under measured semantic floors`
);
console.log(JSON.stringify(summary, null, 2));
