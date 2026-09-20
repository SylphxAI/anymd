#!/usr/bin/env bun

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';
import {
  canonicalTrustReportResult,
  TRUST_REPORT_DEPENDENCY_SURFACES,
  TRUST_REPORT_MUTATION_MANIFEST,
  type Json,
} from './v3014-trust-report-projection.ts';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-trust-report-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-trust-report-oracle.json');
const fixtureManifestPath = join(scriptDir, 'fixtures/v3014-trust-report-fixture.json');
const runnerPath = join(scriptDir, 'v3014-trust-report-baseline-runner.ts');
const projectionPath = join(scriptDir, 'v3014-trust-report-projection.ts');
const generatorPath = join(scriptDir, 'generate-v3014-trust-report-fixture.ts');
const rustCliPath = join(resolveCargoReleaseDir(repoRoot), 'pdf-reader-cli');
const outputFlag = process.argv.indexOf('--output');
const outputPath = outputFlag >= 0 ? process.argv[outputFlag + 1] : undefined;

type Case = { id: string; input: Record<string, unknown> };
type Oracle = {
  baseline: {
    tag: string; commit: string; tree: string; bunLockSha256: string;
    runnerSha256: string; projectionSha256: string; generatorSha256: string;
    corpusSha256: string; fixtureManifestSha256: string; fixtureSha256: string;
    entrypointSha256: Record<string, string>;
  };
  expectations: Record<string, Json>;
};
const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as { cases: Case[] };
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as Oracle;
const fixtureManifest = JSON.parse(readFileSync(fixtureManifestPath, 'utf8')) as {
  fixture: { path: string; bytes: number; sha256: string };
};
const sha256 = (value: Uint8Array | string): string => createHash('sha256').update(value).digest('hex');
const git = (...args: string[]): Buffer => {
  const result = spawnSync('git', args, { cwd: repoRoot });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};
const canonicalJson = (value: Json): Json => {
  if (Array.isArray(value)) return value.map(canonicalJson);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a.localeCompare(b))
      .map(([key, entry]) => [key, canonicalJson(entry)]));
  }
  return value;
};
const same = (left: Json, right: Json): boolean =>
  JSON.stringify(canonicalJson(left)) === JSON.stringify(canonicalJson(right));

const expectedIds = [
  'trust-only-hidden-dependencies', 'trust-reuses-exposed-dependencies',
  'trust-standard-redaction', 'trust-strict-redaction', 'trust-off-redaction',
  'trust-selected-pages-dedupe-sort', 'trust-empty-page-routing',
  'trust-safe-and-unsafe-links', 'trust-document-map-linkage',
];
function verifyAuthority(): Record<string, string> {
  const commit = git('rev-list', '-n', '1', oracle.baseline.tag).toString().trim();
  if (commit !== oracle.baseline.commit) throw new Error('baseline tag moved');
  const tree = git('rev-parse', `${commit}^{tree}`).toString().trim();
  if (tree !== oracle.baseline.tree) throw new Error('baseline tree mismatch');
  if (sha256(git('show', `${commit}:bun.lock`)) !== oracle.baseline.bunLockSha256) throw new Error('baseline lock mismatch');
  for (const [path, expected, label] of [
    [runnerPath, oracle.baseline.runnerSha256, 'runner'],
    [projectionPath, oracle.baseline.projectionSha256, 'projection'],
    [generatorPath, oracle.baseline.generatorSha256, 'generator'],
    [corpusPath, oracle.baseline.corpusSha256, 'corpus'],
    [fixtureManifestPath, oracle.baseline.fixtureManifestSha256, 'fixture manifest'],
  ] as const) {
    if (sha256(readFileSync(path)) !== expected) throw new Error(`trust-report ${label} mismatch`);
  }
  for (const [sourceFile, expected] of Object.entries(oracle.baseline.entrypointSha256)) {
    if (sha256(git('show', `${commit}:${sourceFile}`)) !== expected) throw new Error(`baseline entrypoint mismatch: ${sourceFile}`);
  }
  const fixturePath = join(repoRoot, fixtureManifest.fixture.path);
  if (!existsSync(fixturePath)) throw new Error('missing trust-report fixture');
  const fixtureBytes = readFileSync(fixturePath);
  if (fixtureBytes.length !== fixtureManifest.fixture.bytes || sha256(fixtureBytes) !== fixtureManifest.fixture.sha256 ||
      fixtureManifest.fixture.sha256 !== oracle.baseline.fixtureSha256) throw new Error('trust-report fixture digest mismatch');
  if (JSON.stringify(corpus.cases.map(({ id }) => id)) !== JSON.stringify(expectedIds) ||
      JSON.stringify(Object.keys(oracle.expectations).sort()) !== JSON.stringify([...expectedIds].sort())) {
    throw new Error('trust-report corpus and oracle must contain the exact nine case IDs');
  }
  const oracleText = JSON.stringify(oracle.expectations);
  for (const field of [
    'version', 'profile', 'risk', 'score', 'summary', 'page_reports', 'signals', 'guidance',
    'redaction_policy', 'signal_type_counts', 'safety_finding_type_counts', 'content_safety',
    'layout_uncertainty', 'external_link', 'unsafe_external_link', 'snippet_redacted',
    'redaction_types', 'trust_report_page_index', 'trust_signal_indexes',
  ]) if (!oracleText.includes(`"${field}"`)) throw new Error(`oracle lacks claimed trust-report field: ${field}`);
  return {
    baselineCommit: commit, baselineTree: tree,
    corpusSha256: sha256(readFileSync(corpusPath)), oracleSha256: sha256(readFileSync(oraclePath)),
    runnerSha256: sha256(readFileSync(runnerPath)), projectionSha256: sha256(readFileSync(projectionPath)),
    generatorSha256: sha256(readFileSync(generatorPath)),
    fixtureManifestSha256: sha256(readFileSync(fixtureManifestPath)), fixtureSha256: fixtureManifest.fixture.sha256,
  };
}
function materialize(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(materialize);
  if (value && typeof value === 'object') return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, entry]) => [
      key === 'fixture' ? 'path' : key,
      key === 'fixture' && typeof entry === 'string' ? join(fixtureDir, entry) : materialize(entry),
    ])
  );
  return value;
}
function leafPaths(value: Json, prefix: Array<string | number> = []): Array<Array<string | number>> {
  if (Array.isArray(value)) return value.flatMap((entry, index) => leafPaths(entry, [...prefix, index]));
  if (value && typeof value === 'object') return Object.entries(value).flatMap(([key, entry]) => leafPaths(entry, [...prefix, key]));
  return [prefix];
}
function mutateAt(value: Json, path: Array<string | number>): Json {
  const mutated = structuredClone(value);
  let cursor = mutated as Json;
  for (const segment of path.slice(0, -1)) cursor = (cursor as never)[segment as never];
  const key = path.at(-1)!;
  const original = (cursor as never)[key as never] as Json;
  const replacement: Json = typeof original === 'string' ? `${original}-mutated`
    : typeof original === 'number' ? original + 1
    : typeof original === 'boolean' ? !original : original === null ? 'mutated' : 'mutated';
  (cursor as never)[key as never] = replacement as never;
  return mutated;
}

const authority = verifyAuthority();
if (!existsSync(rustCliPath)) throw new Error('missing release Rust CLI');
const requireSuccessfulSubprocess = (result: ReturnType<typeof spawnSync>, label: string): void => {
  if (result.status !== 0) throw new Error(result.stderr?.toString() || `${label} exited nonzero`);
};
let subprocessNonzeroRejected = false;
try {
  requireSuccessfulSubprocess(spawnSync(process.execPath, ['-e', 'process.exit(17)'], { encoding: 'utf8' }), 'nonzero rejection probe');
} catch { subprocessNonzeroRejected = true; }
if (!subprocessNonzeroRejected) throw new Error('nonzero subprocess was not rejected');

const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
const observations: Record<string, Json> = {};
const rawResults: Record<string, Record<string, unknown>> = {};
for (const entry of corpus.cases) {
  const result = spawnSync(rustCliPath, [], {
    cwd: repoRoot, encoding: 'utf8', input: JSON.stringify({ tool: 'read_pdf', input: materialize(entry.input) }),
    maxBuffer: 120 * 1024 * 1024,
  });
  requireSuccessfulSubprocess(result, `Rust trust-report case: ${entry.id}`);
  const envelope = JSON.parse(result.stdout) as Record<string, unknown>;
  const text = (envelope.result as { content?: Array<{ text?: string }> } | undefined)?.content?.[0]?.text;
  if (!text) throw new Error('Rust CLI response lacks structured text');
  const payload = JSON.parse(text) as { results: Array<{ success: boolean; data: Record<string, unknown> }> };
  if (!payload.results[0]?.success) throw new Error(`Rust trust-report case failed: ${entry.id}`);
  rawResults[entry.id] = payload.results[0].data;
  const actual = canonicalTrustReportResult(payload.results[0].data);
  const expected = oracle.expectations[entry.id]!;
  observations[entry.id] = actual;
  if (!same(actual, expected)) failures.push({ id: entry.id, expected, actual });
}

let leafMutationCount = 0;
for (const [caseId, expectation] of Object.entries(oracle.expectations)) {
  const paths = leafPaths(expectation);
  const missed = paths.filter((path) => same(mutateAt(expectation, path), expectation));
  if (missed.length > 0) throw new Error(`comparison missed ${missed.length} leaf mutations in ${caseId}`);
  leafMutationCount += paths.length;
}
const baseRaw = rawResults['trust-only-hidden-dependencies']!;
const standardRaw = rawResults['trust-standard-redaction']!;
const linksRaw = rawResults['trust-safe-and-unsafe-links']!;
const mapRaw = rawResults['trust-document-map-linkage']!;
const trust = (value: Record<string, unknown>) => value.trust_report as Record<string, unknown>;
const summary = (value: Record<string, unknown>) => trust(value).summary as Record<string, unknown>;
const firstPage = (value: Record<string, unknown>) => (trust(value).page_reports as Array<Record<string, unknown>>)[0]!;
const firstSignal = (value: Record<string, unknown>) => (trust(value).signals as Array<Record<string, unknown>>)[0]!;
const firstTrustMapPage = (value: Record<string, unknown>) =>
  ((value.document_map as Record<string, unknown>).pages as Array<Record<string, unknown>>)[0]!;
const assertRejects = (mutate: (value: Record<string, unknown>) => void, label: string, source = standardRaw): void => {
  const mutated = structuredClone(source);
  mutate(mutated);
  try { canonicalTrustReportResult(mutated); } catch { return; }
  throw new Error(`strict projection accepted invalid mutation: ${label}`);
};
assertRejects((value) => { trust(value).version = 1; }, 'trust version type');
assertRejects((value) => { trust(value).score = '40'; }, 'trust score type');
assertRejects((value) => { (summary(value).selected_pages as unknown[])[0] = '2'; }, 'selected page type');
assertRejects((value) => { summary(value).signal_count = '3'; }, 'summary count type');
assertRejects((value) => { firstPage(value).page = '2'; }, 'page report page type');
assertRejects((value) => { firstSignal(value).severity = 1; }, 'signal severity type');
assertRejects((value) => { (firstTrustMapPage(value).trust_high_signal_indexes as unknown[])[0] = '5'; }, 'map high signal index type', mapRaw);
assertRejects((value) => { firstTrustMapPage(value).trust_medium_signal_indexes = 'none'; }, 'map medium signal indexes type', mapRaw);
assertRejects((value) => { (firstTrustMapPage(value).trust_low_signal_indexes as unknown[])[0] = '4'; }, 'map low signal index type', mapRaw);
assertRejects((value) => { firstTrustMapPage(value).trust_high_signal_count = '1'; }, 'map high signal count type', mapRaw);
assertRejects((value) => { firstTrustMapPage(value).trust_medium_signal_count = '0'; }, 'map medium signal count type', mapRaw);
assertRejects((value) => { firstTrustMapPage(value).trust_low_signal_count = '1'; }, 'map low signal count type', mapRaw);
assertRejects((value) => { trust(value).unexpected = true; }, 'unexpected trust field');
assertRejects((value) => { summary(value).unexpected = true; }, 'unexpected summary field');
assertRejects((value) => { firstPage(value).unexpected = true; }, 'unexpected page report field');
assertRejects((value) => { firstSignal(value).unexpected = true; }, 'unexpected signal field');
assertRejects((value) => { (firstSignal(value).evidence as Record<string, unknown>).unexpected = true; }, 'unexpected evidence field');
assertRejects((value) => { delete trust(value).version; }, 'required version');
assertRejects((value) => { delete trust(value).summary; }, 'required summary');
assertRejects((value) => { delete summary(value).signal_count; }, 'required summary signal count');
assertRejects((value) => { delete trust(value).page_reports; }, 'required page reports');
assertRejects((value) => { delete trust(value).signals; }, 'required signals');
assertRejects((value) => { delete trust(value).guidance; }, 'required guidance');
assertRejects((value) => { delete firstTrustMapPage(value).trust_high_signal_indexes; }, 'required map high signal indexes', mapRaw);
assertRejects((value) => { delete firstTrustMapPage(value).trust_medium_signal_indexes; }, 'required map medium signal indexes', mapRaw);
assertRejects((value) => { delete firstTrustMapPage(value).trust_low_signal_indexes; }, 'required map low signal indexes', mapRaw);
assertRejects((value) => { delete firstTrustMapPage(value).trust_high_signal_count; }, 'required map high signal count', mapRaw);
assertRejects((value) => { delete firstTrustMapPage(value).trust_medium_signal_count; }, 'required map medium signal count', mapRaw);
assertRejects((value) => { delete firstTrustMapPage(value).trust_low_signal_count; }, 'required map low signal count', mapRaw);

let privateLeakProbeCount = 0;
const baseExpected = oracle.expectations['trust-only-hidden-dependencies']!;
for (const key of TRUST_REPORT_MUTATION_MANIFEST.privateLeakage) {
  const mutated = structuredClone(baseRaw);
  mutated[key] = { leaked: true };
  if (same(canonicalTrustReportResult(mutated), baseExpected)) throw new Error(`private output leakage undetected: ${key}`);
  privateLeakProbeCount += 1;
}

const standardEvidence = (trust(standardRaw).signals as Array<Record<string, unknown>>)
  .map((entry) => entry.evidence as Record<string, unknown>);
const strictEvidence = (trust(rawResults['trust-strict-redaction']!).signals as Array<Record<string, unknown>>)[0]!.evidence as Record<string, unknown>;
const offEvidence = (trust(rawResults['trust-off-redaction']!).signals as Array<Record<string, unknown>>)[0]!.evidence as Record<string, unknown>;
const standardText = standardEvidence.map((entry) => String(entry.snippet ?? '')).join(' ');
const standardTypes = new Set(standardEvidence.flatMap((entry) => (entry.redaction_types as string[] | undefined) ?? []));
const redactionProof = {
  standardSensitiveValuesRemoved: !standardText.includes('jane@example.com') && !standardText.includes('123-45-6789') &&
    !standardText.includes('4111 1111 1111 1111') && !standardText.includes('sk-testsecretvalue1234567890') &&
    !standardText.includes('eyJaaaaaaaaaaaa.eyJbbbbbbbbbbbb.cccccccccccccc') && !standardText.includes('-----BEGIN PRIVATE KEY-----'),
  standardTypesCovered: ['email', 'ssn', 'credit_card', 'secret', 'jwt', 'private_key_marker']
    .every((type) => standardTypes.has(type)),
  strictPhoneAndIpv4Removed: !String(strictEvidence.snippet).includes('+1 (415) 555-2671') &&
    !String(strictEvidence.snippet).includes('192.168.0.10') &&
    ['phone', 'ipv4'].every((type) => ((strictEvidence.redaction_types as string[] | undefined) ?? []).includes(type)),
  offPreservesSnippet: String(offEvidence.snippet).includes('jane@example.com') &&
    String(offEvidence.snippet).includes('123-45-6789') && offEvidence.snippet_redacted === false,
};
if (Object.values(redactionProof).some((value) => !value)) throw new Error(`trust redaction proof failed: ${JSON.stringify(redactionProof)}`);

const linkSignals = trust(linksRaw).signals as Array<Record<string, unknown>>;
const annotationProof = {
  safeExternalLink: linkSignals.some((signal) => signal.type === 'external_link' && signal.severity === 'low' &&
    (signal.evidence as Record<string, unknown>)?.url === 'https://example.com/evidence'),
  unsafeExternalLink: linkSignals.some((signal) => signal.type === 'unsafe_external_link' && signal.severity === 'high' &&
    (signal.evidence as Record<string, unknown>)?.url === 'vbscript:msgbox(1)'),
  unsafeGuidance: (trust(linksRaw).guidance as string[]).some((entry) => entry.includes('unsafe PDF link schemes')),
};
if (Object.values(annotationProof).some((value) => !value)) throw new Error(`annotation trust proof failed: ${JSON.stringify(annotationProof)}`);

const exposedRaw = rawResults['trust-reuses-exposed-dependencies']!;
const exposedFindings = exposedRaw.safety_findings as Array<Record<string, unknown>>;
const exposedSignals = trust(exposedRaw).signals as Array<Record<string, unknown>>;
const emptyRaw = rawResults['trust-empty-page-routing']!;
const emptyLayouts = emptyRaw.layout_diagnostics as Array<Record<string, unknown>>;
const emptySignals = trust(emptyRaw).signals as Array<Record<string, unknown>>;
const linkAnnotations = (linksRaw.annotations as Array<Record<string, unknown>>)
  .flatMap((page) => page.annotations as Array<Record<string, unknown>>);
const dependencyReuse = {
  exposedSafetyReused: exposedFindings.every((finding) => exposedSignals.some((signal) => {
    const evidence = signal.evidence as Record<string, unknown>;
    return signal.type === 'content_safety' && signal.severity === finding.severity &&
      signal.page === finding.page && signal.message === finding.message &&
      signal.element_id === finding.element_id && evidence.finding_type === finding.type &&
      same(evidence.bounding_box as Json, finding.bounding_box as Json);
  })),
  exposedLayoutReused: emptyLayouts.every((layout) => emptySignals.some((signal) => {
    const evidence = signal.evidence as Record<string, unknown>;
    return signal.type === 'layout_uncertainty' && signal.page === layout.page &&
      evidence.profile === layout.profile && evidence.reading_order === layout.reading_order &&
      evidence.confidence === layout.confidence && same(evidence.signals as Json, layout.signals as Json) &&
      (Object.hasOwn(layout, 'warnings')
        ? same(evidence.warnings as Json, layout.warnings as Json)
        : !Object.hasOwn(evidence, 'warnings'));
  })),
  exposedAnnotationsReused: linkAnnotations.every((annotation) => linkSignals.some((signal) => {
    const evidence = signal.evidence as Record<string, unknown>;
    return signal.page === annotation.page && signal.annotation_id === annotation.id &&
      evidence.subtype === annotation.subtype && evidence.url === annotation.url &&
      same(evidence.bounding_box as Json, annotation.bounding_box as Json);
  })),
};
if (Object.values(dependencyReuse).some((value) => !value)) {
  throw new Error(`trust dependency reuse mismatch: ${JSON.stringify(dependencyReuse)}`);
}

const map = mapRaw.document_map as Record<string, unknown>;
const mapPages = map.pages as Array<Record<string, unknown>>;
const pageReports = trust(mapRaw).page_reports as Array<Record<string, unknown>>;
const mapLinkage = {
  trustLayerPresent: (map.layers as string[]).includes('trust_report'),
  pageIndexesResolve: mapPages.every((page) => {
    const index = page.trust_report_page_index;
    return typeof index === 'number' && pageReports[index]?.page === page.page;
  }),
  signalIndexesResolve: mapPages.every((page) => (page.trust_signal_indexes as number[]).every((index) =>
    (trust(mapRaw).signals as unknown[])[index] !== undefined)),
  severityIndexesResolve: mapPages.every((page) => {
    const signals = trust(mapRaw).signals as Array<Record<string, unknown>>;
    return (page.trust_high_signal_indexes as number[]).every((index) => signals[index]?.severity === 'high') &&
      (page.trust_medium_signal_indexes as number[]).every((index) => signals[index]?.severity === 'medium') &&
      (page.trust_low_signal_indexes as number[]).every((index) => signals[index]?.severity === 'low');
  }),
  perPageSeverityCountsMatch: mapPages.every((page) =>
    page.trust_high_signal_count === (page.trust_high_signal_indexes as unknown[]).length &&
    page.trust_medium_signal_count === (page.trust_medium_signal_indexes as unknown[]).length &&
    page.trust_low_signal_count === (page.trust_low_signal_indexes as unknown[]).length),
  positiveLowSignalCovered: mapPages.some((page) => page.page === 1 &&
    page.trust_low_signal_count === 1 && (page.trust_low_signal_indexes as unknown[]).length === 1),
  mediumZeroExact: mapPages.every((page) => page.trust_medium_signal_count === 0 &&
    (page.trust_medium_signal_indexes as unknown[]).length === 0),
  summaryMatches: (map.summary as Record<string, unknown>).trust_signal_count === summary(mapRaw).signal_count &&
    (map.summary as Record<string, unknown>).trust_report_page_count === (trust(mapRaw).page_reports as unknown[]).length,
};
if (Object.values(mapLinkage).some((value) => !value)) throw new Error(`document-map trust linkage failed: ${JSON.stringify(mapLinkage)}`);

const selectedPages = summary(rawResults['trust-selected-pages-dedupe-sort']!).selected_pages;
const selectedPageScope = JSON.stringify(selectedPages) === JSON.stringify([1, 5]);
if (!selectedPageScope) throw new Error('selected trust pages were not sorted and deduplicated');
const hiddenSurfaces = canonicalTrustReportResult(baseRaw) as Record<string, Json>;
const hiddenDependencyBehavior = ['safety_findings', 'layout_diagnostics', 'elements', 'tables', 'annotations']
  .every((key) => (hiddenSurfaces.dependency_surfaces as Record<string, Json>)[key] === false);
if (!hiddenDependencyBehavior) throw new Error('trust-only case leaked hidden dependencies');

const mutationManifestSha256 = sha256(JSON.stringify(TRUST_REPORT_MUTATION_MANIFEST));
const mutationSensitive = {
  allClaimedFields: true,
  manifestVersion: TRUST_REPORT_MUTATION_MANIFEST.version,
  mutationManifestSha256,
  leafMutationCount,
  wrongPrimitiveTypeProbeCount: TRUST_REPORT_MUTATION_MANIFEST.wrongPrimitiveTypes.length,
  unexpectedFieldProbeCount: TRUST_REPORT_MUTATION_MANIFEST.unexpectedFields.length,
  requiredOmissionProbeCount: TRUST_REPORT_MUTATION_MANIFEST.requiredOmissions.length,
  privateLeakProbeCount,
  dependencyPresenceProbeCount: TRUST_REPORT_MUTATION_MANIFEST.dependencyPresence.length,
};
const candidateSha = git('rev-parse', 'HEAD').toString().trim();
if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
  throw new Error(`candidate SHA assertion mismatch: expected ${process.env.CANDIDATE_SHA}, executed ${candidateSha}`);
}
const report = {
  schemaVersion: 1, profile: 'pdf_reader_v3014_trust_report_result', candidateSha,
  ...authority, caseCount: corpus.cases.length, passed: corpus.cases.length - failures.length,
  skipped: 0, subprocessNonzeroRejected, mutationSensitive, hiddenDependencyBehavior,
  selectedPageScope, redactionProof, annotationProof, dependencyReuse, mapLinkage,
  nonClaims: [
    'table-quality signals: the deterministic public fixture does not make either runtime emit a table-quality warning',
    'trust semantics outside the exact nine-case corpus, including overlapping/hidden/tiny/off-page text variants',
    'visual, OCR, provider-fusion, malware-scanning, phishing-classification, and remote-link safety',
    'annotation normalization outside exact Link URI cases and arbitrary action dictionaries',
  ],
  pass: failures.length === 0, observations, failures,
};
const serialized = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) await Bun.write(outputPath, serialized);
console.log(serialized.trimEnd());
if (failures.length > 0) process.exit(1);
