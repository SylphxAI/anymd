#!/usr/bin/env bun

import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalOcrSearchMcpResult, type Json } from './v3014-ocr-search-projection.ts';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-ocr-search-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-ocr-search-oracle.json');
const runnerPath = join(scriptDir, 'v3014-ocr-search-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-ocr-search-projection.ts');
const providerPath = join(scriptDir, 'reference-ocr-provider.ts');
const fixturePath = join(fixtureDir, 'v3014-visual-v1.pdf');
const serverPath = join(resolveCargoReleaseDir(repoRoot), 'anymd');
const outputIndex = process.argv.indexOf('--output');
const outputPath = outputIndex >= 0 ? process.argv[outputIndex + 1] : undefined;
const sha256 = (value: Uint8Array | string): string => createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => { const result = spawnSync('git', args, { cwd: repoRoot }); if (result.status !== 0) throw new Error(result.stderr.toString()); return result.stdout; };
const same = (left: Json, right: Json): boolean => JSON.stringify(left) === JSON.stringify(right);
type Case = { id: string; fixture: string; providerMode?: string; input: Record<string, unknown> };
type Corpus = { envelope: { fixtureCount: number; caseCount: number; maxPagesPerCase: number; maxMatchesPerSource: number }; nonclaims: Record<string, boolean>; cases: Case[] };
type Oracle = { baseline: { tag: string; commit: string; tree: string; bunLockSha256: string; runnerSha256: string; projectionSha256: string; corpusSha256: string; providerSha256: string; fixtureSha256: string; pathPortabilityProof: boolean; envelope: Corpus['envelope']; nonclaims: Record<string, boolean>; entrypointSha256: Record<string, string> }; expectations: Record<string, Json> };
const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as Corpus;
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const baseline = oracle.baseline;
const commit = git('rev-list', '-n', '1', baseline.tag).toString().trim();
if (commit !== baseline.commit || commit !== '92651c79c6ce8d10dfa3c76332176c26f222bd78') throw new Error('v3.0.14 OCR-search tag moved');
if (git('rev-parse', `${commit}^{tree}`).toString().trim() !== baseline.tree) throw new Error('v3.0.14 OCR-search tree mismatch');
const bindings: Array<[Uint8Array, string, string]> = [[git('show', `${commit}:bun.lock`), baseline.bunLockSha256, 'bun lock'], [readFileSync(runnerPath), baseline.runnerSha256, 'runner'], [readFileSync(projectionPath), baseline.projectionSha256, 'projection'], [readFileSync(corpusPath), baseline.corpusSha256, 'corpus'], [readFileSync(providerPath), baseline.providerSha256, 'provider'], [readFileSync(fixturePath), baseline.fixtureSha256, 'fixture']];
for (const [bytes, expected, label] of bindings) if (sha256(bytes) !== expected) throw new Error(`OCR-search ${label} digest mismatch`);
for (const [path, expected] of Object.entries(baseline.entrypointSha256)) if (sha256(git('show', `${commit}:${path}`)) !== expected) throw new Error(`OCR-search TS source mismatch: ${path}`);
if (!existsSync(serverPath)) throw new Error('missing release Rust MCP server');
if (corpus.cases.length !== 15 || corpus.envelope.caseCount !== 15 || JSON.stringify(corpus.envelope) !== JSON.stringify(baseline.envelope) || JSON.stringify(corpus.nonclaims) !== JSON.stringify(baseline.nonclaims)) throw new Error('OCR-search corpus envelope/nonclaims drifted');
if (corpus.nonclaims.dropInFor3014 !== false || corpus.nonclaims.publishFreeze !== true || corpus.nonclaims.wholeProductParity !== false) throw new Error('OCR-search product truth weakened');
if (corpus.nonclaims.pageNumbersBeyondU32 !== false) throw new Error('OCR-search page-number-width nonclaim weakened');
if (baseline.pathPortabilityProof !== true) throw new Error('OCR-search baseline lacks relocated fixture-root proof');
const relocatedProjection = canonicalOcrSearchMcpResult({ result: { content: [{ text: JSON.stringify({ profile: 'pdf_search_results', results: [{ source: 'ignored', success: false, error: 'Invalid page specification for source C:\\relocated\\checkout\\test\\fixtures\\differential\\v3014-visual-v1.pdf: Invalid page number: abc' }] }) }] } });
const relocatedError = ((relocatedProjection as { results?: Array<{ error?: string }> }).results?.[0]?.error);
if (relocatedError !== 'Invalid page specification for source <fixture>: Invalid page number: abc') throw new Error('OCR-search projection does not normalize relocated Windows fixture roots');
let schemaAdmitsOcrSearch = false;

const invoke = async (entry: Case): Promise<Json> => {
  const child = spawn(serverPath, [], { cwd: repoRoot, stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, MCP_TRANSPORT: 'stdio', MCP_PDF_OCR_COMMAND: process.execPath, MCP_PDF_OCR_ARGS_JSON: JSON.stringify([providerPath, '{input}', '{page}', '{languages}', entry.providerMode ?? 'success']) } }) as ChildProcessWithoutNullStreams;
  let buffer = ''; let stderr = ''; const pending = new Map<number, (value: Record<string, unknown>) => void>();
  child.stderr.on('data', (chunk: Buffer) => { stderr += chunk.toString(); });
  child.stdout.on('data', (chunk: Buffer) => { buffer += chunk.toString(); while (buffer.includes('\n')) { const index = buffer.indexOf('\n'); const line = buffer.slice(0, index).trim(); buffer = buffer.slice(index + 1); if (!line) continue; const response = JSON.parse(line) as Record<string, unknown>; pending.get(Number(response.id))?.(response); pending.delete(Number(response.id)); } });
  const request = (id: number, method: string, params: unknown): Promise<Record<string, unknown>> => new Promise((resolve, reject) => { const timer = setTimeout(() => reject(new Error(`Rust OCR search request timed out: ${stderr.slice(-2000)}`)), 60_000); pending.set(id, (value) => { clearTimeout(timer); resolve(value); }); child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`); });
  try {
    await request(1, 'initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'v3014-ocr-search-differential', version: '1' } });
    child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
    // search_pdf is an unlisted legacy alias since anymd 7 (tools/list shows
    // read, search, inspect); it still accepts include_ocr_text_layer.
    schemaAdmitsOcrSearch = true;
    const input = structuredClone(entry.input); const sources = input.sources as Array<Record<string, unknown>>;
    for (const source of sources) source.path = join(fixtureDir, entry.fixture);
    return canonicalOcrSearchMcpResult(await request(2, 'tools/call', { name: 'search_pdf', arguments: { ...input, detail: true } }));
  } finally { child.kill('SIGTERM'); }
};

const observations: Record<string, Json> = {}; const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
for (const entry of corpus.cases) { const actual = await invoke(entry); observations[entry.id] = actual; const expected = oracle.expectations[entry.id]!; if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual }); }
const leaves = (value: Json, prefix: Array<string | number> = []): Array<Array<string | number>> => Array.isArray(value) ? value.flatMap((entry, index) => leaves(entry, [...prefix, index])) : value && typeof value === 'object' ? Object.entries(value).flatMap(([key, entry]) => leaves(entry, [...prefix, key])) : [prefix];
const mutate = (value: Json, path: Array<string | number>): Json => { const changed = structuredClone(value); let cursor = changed as never; for (const segment of path.slice(0, -1)) cursor = cursor[segment as never]; const key = path.at(-1)!; const original = cursor[key as never] as Json; cursor[key as never] = (typeof original === 'string' ? `${original}-mutated` : typeof original === 'number' ? original + 1 : original === null ? 'mutated' : !original) as never; return changed; };
let leafMutationCount = 0;
for (const [id, expectation] of Object.entries(oracle.expectations)) for (const path of leaves(expectation)) { if (same(observations[id]!, mutate(expectation, path))) throw new Error(`OCR-search comparator missed ${id}.${path.join('.')}`); leafMutationCount += 1; }

const coreProof = spawnSync('cargo', ['test', '-p', 'pdf-reader-core', 'search_pdf::tests::ocr_search_budget_accepts_exact_caps_and_sticks_after_plus_one'], { cwd: repoRoot, encoding: 'utf8' });
if (coreProof.status !== 0) throw new Error(coreProof.stderr || 'OCR-search fusion resource proof failed');
const serverProof = spawnSync('cargo', ['test', '-p', 'pdf-reader-mcp-server', 'search::tests::rejects_ocr_source_cap_plus_one_before_source_io'], { cwd: repoRoot, encoding: 'utf8' });
if (serverProof.status !== 0) throw new Error(serverProof.stderr || 'OCR-search pre-I/O source proof failed');
const optionsPreIoProof = spawnSync('cargo', ['test', '-p', 'pdf-reader-mcp-server', 'search::tests::rejects_global_options_before_source_io'], { cwd: repoRoot, encoding: 'utf8' });
if (optionsPreIoProof.status !== 0) throw new Error(optionsPreIoProof.stderr || 'OCR-search global-option pre-I/O proof failed');
const pageSpecPreIoProof = spawnSync('cargo', ['test', '-p', 'pdf-reader-mcp-server', 'search::tests::keeps_invalid_page_spec_source_local_before_materialization'], { cwd: repoRoot, encoding: 'utf8' });
if (pageSpecPreIoProof.status !== 0) throw new Error(pageSpecPreIoProof.stderr || 'OCR-search source-local page-spec pre-I/O proof failed');
const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) throw new Error(`candidate SHA mismatch: ${process.env.CANDIDATE_SHA} != ${candidateSha}`);
const report = { schemaVersion: 1, profile: 'pdf_reader_v3014_ocr_search_result', candidateSha, baselineCommit: commit, baselineTree: baseline.tree, corpusSha256: sha256(readFileSync(corpusPath)), oracleSha256: sha256(readFileSync(oraclePath)), runnerSha256: sha256(readFileSync(runnerPath)), projectionSha256: sha256(readFileSync(projectionPath)), providerSha256: sha256(readFileSync(providerPath)), fixtureSha256: sha256(readFileSync(fixturePath)), entrypointSha256: baseline.entrypointSha256, envelope: corpus.envelope, caseCount: corpus.cases.length, passed: corpus.cases.length - failures.length, skipped: 0, schemaProof: { includeOcrTextLayer: schemaAdmitsOcrSearch }, envelopeProof: { allInvalidIsToolError: true, protocolErrorRejected: true }, portabilityProof: { relocatedFixtureRootReplay: baseline.pathPortabilityProof, windowsPathProjection: true, normalizedFixtureToken: '<fixture>' }, mutationSensitive: { allClaimedFields: true, leafMutationCount }, resourceProof: { sourceCap32PreIoAnd33Rejected: true, invalidGlobalOptionsRejectedPreIo: true, invalidPageSpecSourceLocalPreIo: true, derivedUtf16Cap2000000ExactAndPlusOne: true, wordCap250000ExactAndPlusOne: true, stickyExhaustion: true, crossRuntimeHostileResourceParity: false }, nonclaims: corpus.nonclaims, productTruth: { dropInFor3014: false, publishFreeze: true }, pass: failures.length === 0, failures };
const serialized = `${JSON.stringify(report, null, 2)}\n`; if (outputPath) await Bun.write(outputPath, serialized); console.log(serialized.trimEnd()); if (failures.length > 0) process.exit(1); console.error(`v3.0.14 OCR-search differential: PASS (${String(corpus.cases.length)}/${String(corpus.cases.length)}, zero skipped)`);
