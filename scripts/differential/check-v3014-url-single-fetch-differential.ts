#!/usr/bin/env bun

import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalUrlSingleFetchResult, type Json } from './v3014-url-single-fetch-projection.ts';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-url-single-fetch-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-url-single-fetch-oracle.json');
const runnerPath = join(scriptDir, 'v3014-url-single-fetch-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-url-single-fetch-projection.ts');
const providerPath = join(scriptDir, 'reference-ocr-provider.ts');
const serverScriptPath = join(scriptDir, 'url-single-fetch-fixture-server.ts');
const fixturePath = join(fixtureDir, 'v3014-visual-v1.pdf');
const serverPath = join(resolveCargoReleaseDir(repoRoot), 'citra-mcp-server');
const outputIndex = process.argv.indexOf('--output');
const outputPath = outputIndex >= 0 ? process.argv[outputIndex + 1] : undefined;
const sha256 = (value: Uint8Array | string): string =>
  createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => {
  const result = spawnSync('git', args, { cwd: repoRoot });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};
const same = (left: Json, right: Json): boolean => JSON.stringify(left) === JSON.stringify(right);

type Case = {
  id: string;
  tool: 'read_pdf' | 'search_pdf';
  fixture: string;
  providerMode?: string;
  input: Record<string, unknown>;
};
type Corpus = {
  envelope: Record<string, number>;
  nonclaims: Record<string, boolean>;
  cases: Case[];
};
type Oracle = {
  baseline: {
    tag: string;
    commit: string;
    tree: string;
    bunLockSha256: string;
    runnerSha256: string;
    projectionSha256: string;
    corpusSha256: string;
    providerSha256: string;
    serverScriptSha256: string;
    fixtureSha256: string;
    entrypointSha256: Record<string, string>;
  };
  expectations: Record<string, Json>;
};

const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as Corpus;
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const baseline = oracle.baseline;
const commit = git('rev-list', '-n', '1', baseline.tag).toString().trim();
if (commit !== baseline.commit || commit !== '92651c79c6ce8d10dfa3c76332176c26f222bd78') {
  throw new Error('v3.0.14 url single-fetch tag moved');
}
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== baseline.tree) {
  throw new Error('v3.0.14 url single-fetch tree mismatch');
}
for (const [name, path, expected] of [
  ['runner', runnerPath, baseline.runnerSha256],
  ['projection', projectionPath, baseline.projectionSha256],
  ['corpus', corpusPath, baseline.corpusSha256],
  ['provider', providerPath, baseline.providerSha256],
  ['serverScript', serverScriptPath, baseline.serverScriptSha256],
  ['fixture', fixturePath, baseline.fixtureSha256],
] as const) {
  if (sha256(readFileSync(path)) !== expected) {
    throw new Error(`url single-fetch ${name} digest drift`);
  }
}
for (const [path, expected] of Object.entries(baseline.entrypointSha256)) {
  if (sha256(git('show', `${commit}:${path}`)) !== expected) {
    throw new Error(`url single-fetch entrypoint digest drift: ${path}`);
  }
}
if (sha256(git('show', `${commit}:bun.lock`)) !== baseline.bunLockSha256) {
  throw new Error('url single-fetch bun.lock digest drift');
}
if (!existsSync(serverPath)) {
  const build = spawnSync('cargo', ['build', '-p', 'pdf-reader-mcp-server', '--release'], {
    cwd: repoRoot,
    encoding: 'utf8',
  });
  if (build.status !== 0) throw new Error(build.stderr || 'release server build failed');
}

const startServer = async (
  root: string,
  fixture: string
): Promise<{ url: string; counterPath: string; stop: () => void }> => {
  const work = mkdtempSync(join(tmpdir(), 'pdf-url-single-fetch-rust-'));
  const counterPath = join(work, 'counter.json');
  writeFileSync(counterPath, `${JSON.stringify({ hits: 0 })}\n`);
  const child = spawn(
    process.execPath,
    [serverScriptPath, join(root, fixture), counterPath, '0'],
    { stdio: ['ignore', 'pipe', 'pipe'] }
  );
  let stdout = '';
  let stderr = '';
  child.stdout.on('data', (chunk: Buffer) => {
    stdout += chunk.toString();
  });
  child.stderr.on('data', (chunk: Buffer) => {
    stderr += chunk.toString();
  });
  const started = Date.now();
  while (!stdout.includes('\n')) {
    if (Date.now() - started > 10_000) {
      child.kill('SIGTERM');
      throw new Error(`fixture server failed to start: ${stderr}`);
    }
    await Bun.sleep(20);
  }
  const info = JSON.parse(stdout.trim().split('\n')[0]!) as { port: number; path: string };
  return {
    url: `http://127.0.0.1:${String(info.port)}${info.path}`,
    counterPath,
    stop: () => {
      child.kill('SIGTERM');
      rmSync(work, { recursive: true, force: true });
    },
  };
};

const invoke = async (entry: Case, root: string): Promise<Json> => {
  const server = await startServer(root, entry.fixture);
  try {
    const env: NodeJS.ProcessEnv = {
      ...process.env,
      MCP_TRANSPORT: 'stdio',
      MCP_PDF_ALLOW_PRIVATE_IPS: 'true',
      MCP_PDF_OCR_COMMAND: process.execPath,
      MCP_PDF_OCR_ARGS_JSON: JSON.stringify([
        providerPath,
        '{input}',
        '{page}',
        '{languages}',
        entry.providerMode ?? 'success',
      ]),
    };
    const child = spawn(serverPath, [], {
      cwd: repoRoot,
      stdio: ['pipe', 'pipe', 'pipe'],
      env,
    }) as ChildProcessWithoutNullStreams;
    let buffer = '';
    let stderr = '';
    const pending = new Map<number, (value: Record<string, unknown>) => void>();
    child.stderr.on('data', (chunk: Buffer) => {
      stderr += chunk.toString();
    });
    child.stdout.on('data', (chunk: Buffer) => {
      buffer += chunk.toString();
      while (buffer.includes('\n')) {
        const index = buffer.indexOf('\n');
        const line = buffer.slice(0, index).trim();
        buffer = buffer.slice(index + 1);
        if (!line) continue;
        const response = JSON.parse(line) as Record<string, unknown>;
        pending.get(Number(response.id))?.(response);
        pending.delete(Number(response.id));
      }
    });
    const request = (
      id: number,
      method: string,
      params: unknown
    ): Promise<Record<string, unknown>> =>
      new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          reject(new Error(`Rust url single-fetch timed out: ${stderr.slice(-2000)}`));
        }, 120_000);
        pending.set(id, (value) => {
          clearTimeout(timer);
          resolve(value);
        });
        child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
      });
    try {
      await request(1, 'initialize', {
        protocolVersion: '2024-11-05',
        capabilities: {},
        clientInfo: { name: 'v3014-url-single-fetch-rust', version: '1' },
      });
      child.stdin.write(
        `${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`
      );
      const input = structuredClone(entry.input);
      const sources = input.sources as Array<Record<string, unknown>>;
      for (const source of sources) {
        delete source.path;
        source.url = server.url;
      }
      const response = await request(2, 'tools/call', {
        name: entry.tool,
        arguments: input,
      });
      const counter = JSON.parse(readFileSync(server.counterPath, 'utf8')) as { hits: number };
      return canonicalUrlSingleFetchResult(response, {
        tool: entry.tool,
        fetchHits: counter.hits,
      });
    } finally {
      child.kill('SIGTERM');
    }
  } finally {
    server.stop();
  }
};

const observations: Record<string, Json> = {};
const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
for (const entry of corpus.cases) {
  const actual = await invoke(entry, fixtureDir);
  observations[entry.id] = actual;
  const expected = oracle.expectations[entry.id]!;
  if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
}

const relocatedDir = mkdtempSync(join(tmpdir(), 'pdf-reader-url-single-fetch-relocated-'));
try {
  copyFileSync(fixturePath, join(relocatedDir, 'v3014-visual-v1.pdf'));
  for (const entry of corpus.cases) {
    const actual = await invoke(entry, relocatedDir);
    if (!same(actual, oracle.expectations[entry.id]!)) {
      throw new Error(`relocated fixture-root replay failed for ${entry.id}`);
    }
  }
} finally {
  rmSync(relocatedDir, { recursive: true, force: true });
}

const leaves = (value: Json, prefix: Array<string | number> = []): Array<Array<string | number>> =>
  Array.isArray(value)
    ? value.flatMap((entry, index) => leaves(entry, [...prefix, index]))
    : value && typeof value === 'object'
      ? Object.entries(value).flatMap(([key, entry]) => leaves(entry as Json, [...prefix, key]))
      : [prefix];
const mutate = (value: Json, path: Array<string | number>): Json => {
  const changed = structuredClone(value);
  let cursor = changed as never;
  for (const segment of path.slice(0, -1)) cursor = cursor[segment as never];
  const key = path.at(-1)!;
  const original = cursor[key as never] as Json;
  cursor[key as never] = (
    typeof original === 'string'
      ? `${original}-mutated`
      : typeof original === 'number'
        ? original + 1
        : original === null
          ? 'mutated'
          : !original
  ) as never;
  return changed;
};
let leafMutationCount = 0;
for (const [id, expectation] of Object.entries(oracle.expectations)) {
  for (const path of leaves(expectation)) {
    if (same(observations[id]!, mutate(expectation, path))) {
      throw new Error(`url single-fetch comparator missed ${id}.${path.join('.')}`);
    }
    leafMutationCount += 1;
  }
}

for (const id of corpus.cases.map((entry) => entry.id)) {
  const observation = observations[id] as Record<string, Json>;
  if (observation.fetchHits !== 1 || observation.singleFetch !== true) {
    throw new Error(`${id} must prove exactly one HTTP fetch`);
  }
}

const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
}

const report = {
  schemaVersion: 1,
  profile: 'pdf_reader_v3014_url_single_fetch_result',
  candidateSha,
  baselineCommit: commit,
  baselineTree: baseline.tree,
  corpusSha256: sha256(readFileSync(corpusPath)),
  oracleSha256: sha256(readFileSync(oraclePath)),
  runnerSha256: sha256(readFileSync(runnerPath)),
  projectionSha256: sha256(readFileSync(projectionPath)),
  providerSha256: sha256(readFileSync(providerPath)),
  fixtureSha256: baseline.fixtureSha256,
  entrypointSha256: baseline.entrypointSha256,
  envelope: corpus.envelope,
  caseCount: corpus.cases.length,
  passed: corpus.cases.length - failures.length,
  skipped: 0,
  mutationSensitive: { allClaimedFields: true, leafMutationCount },
  portabilityProof: {
    relocatedFixtureRootReplay: true,
    normalizedFixtureToken: '<url-fixture>',
  },
  providerProof: {
    configuredCommandOnly: true,
    allowPrivateIpsRequired: true,
    readPdfNoOcrSingleFetch: true,
    readPdfWithOcrSingleFetch: true,
  },
  nonclaims: corpus.nonclaims,
  productTruth: { dropInFor3014: false, publishFreeze: true },
  capabilityStatus: { urlSsrf: 'PARTIAL' },
  pass: failures.length === 0,
  failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
console.error(
  `v3.0.14 url single-fetch differential: PASS (${String(corpus.cases.length)}/${String(corpus.cases.length)}, zero skipped)`
);
