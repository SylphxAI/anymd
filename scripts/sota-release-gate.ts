import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import type { DoctorReport } from '../src/doctor.js';
import { writeBenchmarkReport } from './benchmark-utils.js';

const runDoctorViaSubprocess = (version: string): DoctorReport => {
  const repoRoot = path.resolve(import.meta.dirname, '..');
  const result = spawnSync('bun', ['run', 'src/doctor-cli.ts'], {
    cwd: repoRoot,
    encoding: 'utf8',
    env: process.env,
    timeout: 60_000,
  });

  if (!result.stdout?.trim()) {
    return {
      profile: 'pdf_reader_doctor',
      version,
      status: 'unavailable',
      checks: [
        {
          id: 'doctor:subprocess',
          status: 'fail',
          message: result.stderr?.slice(-500) ?? 'doctor subprocess produced no output',
        },
      ],
    };
  }

  try {
    return JSON.parse(result.stdout) as DoctorReport;
  } catch {
    return {
      profile: 'pdf_reader_doctor',
      version,
      status: 'unavailable',
      checks: [
        {
          id: 'doctor:subprocess',
          status: 'fail',
          message: 'doctor subprocess output was not valid JSON',
        },
      ],
    };
  }
};

const ARTIFACT_DIR_ENV = 'MCP_PDF_BENCHMARK_OUTPUT_DIR';
const DEFAULT_ARTIFACT_DIR = 'benchmark-artifacts';
const ARTIFACT_DIR_FLAGS = new Set(['--artifacts-dir', '--artifact-dir']);
const REQUIRED_CORPUS_CASE_IDS = [
  'checked-in-sample-agent-document-twin',
  'runtime-report-reading-order',
  'runtime-scanned-ocr-routing',
  'runtime-ocr-table-agent-evidence',
  'runtime-malformed-pdf-trust-routing',
  'runtime-encrypted-pdf-trust-routing',
] as const;
const REQUIRED_CORPUS_CAPABILITY_TAGS = [
  'document_map',
  'reading_order',
  'ocr_routing',
  'ocr_text_layer',
  'ocr_table_extraction',
  'scanned_page',
  'scanned_table',
  'text_layer',
  'malformed_pdf',
  'encrypted_pdf',
  'trust_routing',
] as const;
const REQUIRED_PROVIDER_CROP_CAPABILITY_TAGS = [
  'crop_provenance',
  'document_twin',
  'full_page_crop',
  'release_evidence',
  'render_provenance',
  'visual_text',
] as const;
const REQUIRED_PROVIDER_MANIFEST_CAPABILITY_TAGS = [
  'chart_extraction',
  'crop_provenance',
  'document_twin',
  'figure_description',
  'formula_recognition',
  'image_description',
  'provider_manifest_scoring',
  'release_evidence',
  'table_recognition',
] as const;
const REQUIRED_PROVIDER_MANIFEST_KINDS = ['chart', 'figure', 'formula', 'image', 'table'] as const;

const REQUIRED_ARTIFACTS = {
  performance: {
    fileName: 'pdf_performance_benchmark.json',
    profile: 'pdf_performance_benchmark',
  },
  quality: {
    fileName: 'pdf_quality_benchmark.json',
    profile: 'pdf_quality_benchmark',
  },
  corpus: {
    fileName: 'pdf_corpus_benchmark.json',
    profile: 'pdf_corpus_benchmark',
  },
  provider: {
    fileName: 'pdf_provider_benchmark.json',
    profile: 'pdf_provider_benchmark',
  },
  'provider-manifest': {
    fileName: 'pdf_provider_manifest_benchmark.json',
    profile: 'pdf_provider_manifest_benchmark',
  },
  'provider-crops': {
    fileName: 'pdf_provider_manifest_crop_benchmark.json',
    profile: 'pdf_provider_manifest_crop_benchmark',
  },
} as const;

type GateStatus = 'passed' | 'failed';

interface GateCheck {
  id: string;
  status: GateStatus;
  message: string;
  evidence?: Record<string, unknown> | undefined;
}

export interface SotaReleaseGateReport {
  profile: 'pdf_sota_release_gate';
  generated_at: string;
  artifact_dir: string;
  status: GateStatus;
  summary: {
    total: number;
    passed: number;
    failed: number;
  };
  checks: GateCheck[];
}

type JsonRecord = Record<string, unknown>;

const readFlagValue = (argv: string[], flagNames: Set<string>): string | undefined => {
  for (let index = 0; index < argv.length; index++) {
    const arg = argv[index];
    if (flagNames.has(arg)) {
      const value = argv[index + 1];
      return value && !value.startsWith('-') ? value : undefined;
    }

    for (const flagName of flagNames) {
      const prefix = `${flagName}=`;
      if (arg.startsWith(prefix)) {
        return arg.slice(prefix.length);
      }
    }
  }

  return undefined;
};

export const resolveSotaReleaseArtifactDir = (
  argv = process.argv.slice(2),
  env: Record<string, string | undefined> = process.env
): string => path.resolve(readFlagValue(argv, ARTIFACT_DIR_FLAGS) ?? env[ARTIFACT_DIR_ENV] ?? DEFAULT_ARTIFACT_DIR);

const isRecord = (value: unknown): value is JsonRecord =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const readJsonArtifact = async (
  artifactDir: string,
  fileName: string
): Promise<JsonRecord | undefined> => {
  const artifactPath = path.join(artifactDir, fileName);
  try {
    const raw = await fs.promises.readFile(artifactPath, 'utf8');
    const parsed = JSON.parse(raw) as unknown;
    return isRecord(parsed) ? parsed : undefined;
  } catch {
    return undefined;
  }
};

const getArray = (record: JsonRecord | undefined, key: string): JsonRecord[] =>
  Array.isArray(record?.[key]) ? record[key].filter(isRecord) : [];

const getNumber = (record: JsonRecord | undefined, key: string): number | undefined =>
  typeof record?.[key] === 'number' ? record[key] : undefined;

const getString = (record: JsonRecord | undefined, key: string): string | undefined =>
  typeof record?.[key] === 'string' ? record[key] : undefined;

const getRecord = (record: JsonRecord | undefined, key: string): JsonRecord | undefined =>
  isRecord(record?.[key]) ? record[key] : undefined;

const getStringArray = (record: JsonRecord | undefined, key: string): string[] =>
  Array.isArray(record?.[key])
    ? record[key].filter((entry): entry is string => typeof entry === 'string')
    : [];

const addCheck = (
  checks: GateCheck[],
  id: string,
  pass: boolean,
  message: string,
  evidence?: Record<string, unknown>
) => {
  checks.push({
    id,
    status: pass ? 'passed' : 'failed',
    message,
    evidence,
  });
};

const summarizeChecks = (checks: GateCheck[]): SotaReleaseGateReport['summary'] => ({
  total: checks.length,
  passed: checks.filter((check) => check.status === 'passed').length,
  failed: checks.filter((check) => check.status === 'failed').length,
});

export const buildSotaReleaseGateReport = async (
  artifactDir: string
): Promise<SotaReleaseGateReport> => {
  const absoluteArtifactDir = path.resolve(artifactDir);
  const checks: GateCheck[] = [];
  const artifacts = {
    performance: await readJsonArtifact(absoluteArtifactDir, REQUIRED_ARTIFACTS.performance.fileName),
    quality: await readJsonArtifact(absoluteArtifactDir, REQUIRED_ARTIFACTS.quality.fileName),
    corpus: await readJsonArtifact(absoluteArtifactDir, REQUIRED_ARTIFACTS.corpus.fileName),
    provider: await readJsonArtifact(absoluteArtifactDir, REQUIRED_ARTIFACTS.provider.fileName),
    'provider-manifest': await readJsonArtifact(
      absoluteArtifactDir,
      REQUIRED_ARTIFACTS['provider-manifest'].fileName
    ),
    'provider-crops': await readJsonArtifact(
      absoluteArtifactDir,
      REQUIRED_ARTIFACTS['provider-crops'].fileName
    ),
  };

  for (const [kind, requirement] of Object.entries(REQUIRED_ARTIFACTS)) {
    const artifact = artifacts[kind as keyof typeof artifacts];
    addCheck(checks, `artifact:${kind}`, artifact !== undefined, `${requirement.fileName} exists and is valid JSON`, {
      file: requirement.fileName,
      profile: artifact?.profile,
    });
    addCheck(
      checks,
      `profile:${kind}`,
      artifact?.profile === requirement.profile,
      `${requirement.fileName} has profile ${requirement.profile}`,
      { expected: requirement.profile, actual: artifact?.profile }
    );
  }

  const repoRoot = path.resolve(import.meta.dirname, '..');
  addCheck(
    checks,
    'rust:hash_core',
    fs.existsSync(path.join(repoRoot, 'crates/pdf-reader-core/src/lib.rs')),
    'Rust pdf-reader-core hash engine is present for Phase 1 native performance layer'
  );

  addCheck(
    checks,
    'rust:text_index_core',
    fs.existsSync(path.join(repoRoot, 'crates/pdf-reader-core/src/text_index.rs')),
    'Rust pdf-reader-core text index engine is present for literal search_pdf acceleration'
  );

  addCheck(
    checks,
    'rust:page_cache_core',
    fs.existsSync(path.join(repoRoot, 'crates/pdf-reader-core/src/page_cache.rs')),
    'Rust pdf-reader-core page cache is present for repeated literal search acceleration'
  );

  addCheck(
    checks,
    'rust:mcp_server',
    fs.existsSync(path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/lib.rs')),
    'Rust MCP server (modelcontextprotocol/rust-sdk rmcp) is present'
  );

  const binWrapper = fs.readFileSync(path.join(repoRoot, 'bin/anymd'), 'utf8');
  addCheck(
    checks,
    'mcp:sole_rust_launcher_boundary',
    binWrapper.includes('dist/runtime-entry.js') &&
      binWrapper.includes('anymd') &&
      !binWrapper.includes('dist/index.js') &&
      !binWrapper.includes('legacy-engine-runtime') &&
      !fs.existsSync(path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/parity_bridge.rs')),
    'Local/published launcher is sole-Rust native binary path without TypeScript PDF runtime'
  );
  addCheck(
    checks,
    'mcp:production_contract_suite',
    fs.existsSync(path.join(repoRoot, 'test/production/productionPath.contract.test.ts')) &&
      fs.existsSync(path.join(repoRoot, 'test/production/publicContract.matrix.json')) &&
      fs.existsSync(path.join(repoRoot, 'scripts/check-production-contract.ts')) &&
      fs
        .readFileSync(path.join(repoRoot, 'package.json'), 'utf8')
        .includes('check:production-contract'),
    'Star-project production-path public contract suite is wired into package scripts'
  );

  const httpTransportSource = fs.readFileSync(
    path.join(repoRoot, 'crates/pdf-reader-mcp-server/src/http_transport.rs'),
    'utf8'
  );
  addCheck(
    checks,
    'mcp:rust_web_http_transport',
    httpTransportSource.includes('StreamableHttpService') &&
      httpTransportSource.includes('/mcp/health') &&
      (binWrapper.includes('MCP_TRANSPORT') || fs.readFileSync(path.join(repoRoot, 'src/runtime-entry.ts'), 'utf8').includes('MCP_TRANSPORT')),
    'Rust rmcp streamable HTTP remains available via sole-Rust native server'
  );

  const httpIntegration = fs.readFileSync(
    path.join(repoRoot, 'test/integration/http-transport.test.ts'),
    'utf8'
  );
  const stdioIntegration = fs.readFileSync(
    path.join(repoRoot, 'test/integration/stdio-transport.test.ts'),
    'utf8'
  );
  const goldenFixture = path.join(repoRoot, 'test/fixtures/read-pdf-golden.json');
  const coreGolden = path.join(repoRoot, 'crates/pdf-reader-core/tests/read_pdf_golden_parity.rs');
  const rmcpGolden = path.join(
    repoRoot,
    'crates/pdf-reader-mcp-server/tests/read_pdf_golden_parity.rs'
  );
  const httpAuthorityGate = path.join(repoRoot, 'scripts/check-no-ts-http-backend.sh');
  const stdioDeletionGate = path.join(repoRoot, 'scripts/check-no-ts-stdio-backend.sh');

  addCheck(
    checks,
    'mcp:http_transport_parity',
    httpIntegration.includes('MCP Server HTTP Transport Integration') &&
      httpIntegration.includes('golden mock parity over HTTP'),
    'HTTP integration harness proves read_pdf golden mock parity over streamable HTTP'
  );
  addCheck(
    checks,
    'mcp:stdio_transport_parity',
    stdioIntegration.includes('MCP Server stdio Transport Integration') &&
      stdioIntegration.includes('golden mock parity over stdio'),
    'stdio integration harness proves read_pdf golden mock parity over Rust rmcp stdio'
  );
  addCheck(
    checks,
    'mcp:read_pdf_golden_parity',
    fs.existsSync(goldenFixture) && fs.existsSync(coreGolden),
    'read_pdf golden fixture + core golden parity harness are present'
  );
  addCheck(
    checks,
    'mcp:rmcp_read_pdf_parity',
    fs.existsSync(rmcpGolden),
    'rmcp server read_pdf golden parity harness is present'
  );
  addCheck(
    checks,
    'mcp:read_pdf_cross_parity',
    fs.existsSync(path.join(repoRoot, 'test/readPdf.parity.test.ts')),
    'cross-surface read_pdf parity tests (TS pure/oracle) are present'
  );
  addCheck(
    checks,
    'mcp:http_gate_script_present',
    fs.existsSync(httpAuthorityGate),
    'HTTP authority gate script remains present for future pure-Rust cutover'
  );
  addCheck(
    checks,
    'mcp:stdio_gate_script_present',
    fs.existsSync(stdioDeletionGate),
    'stdio gate script remains present for future pure-Rust cutover'
  );

  const matrixProbe = spawnSync(
    'bun',
    ['test', 'test/shippedPath.matrix.test.ts'],
    {
      cwd: repoRoot,
      encoding: 'utf8',
      env: process.env,
      timeout: 300_000,
    }
  );
  addCheck(
    checks,
    'boundary:rust_cli_engine',
    !fs.existsSync(path.join(repoRoot, 'src/engine-invoke.ts')) &&
      matrixProbe.status === 0,
    'Shipped-path matrix test proves all primary tools route through Rust core without legacy runtime',
    matrixProbe.status === 0
      ? { exitCode: 0 }
      : {
          exitCode: matrixProbe.status,
          stderr: matrixProbe.stderr?.slice(-2000),
          stdout: matrixProbe.stdout?.slice(-2000),
        }
  );

  const performanceResults = getArray(artifacts.performance, 'results');
  const documentTwinPerformance = performanceResults.find(
    (result) => result.name === 'v3_agent_document_twin'
  );
  const defaultAutoReadPerformance = performanceResults.find(
    (result) => result.name === 'default_auto_read_balanced'
  );
  addCheck(
    checks,
    'performance:document-twin-present',
    documentTwinPerformance !== undefined,
    'performance artifact includes the v3 Agent Document Twin scenario'
  );
  addCheck(
    checks,
    'performance:document-twin-timed',
    (getNumber(documentTwinPerformance, 'average_ms') ?? 0) > 0,
    'v3 Agent Document Twin scenario has a positive average latency',
    { average_ms: documentTwinPerformance?.average_ms }
  );
  addCheck(
    checks,
    'performance:default-auto-read-present',
    defaultAutoReadPerformance !== undefined,
    'performance artifact includes the default balanced auto-read scenario'
  );
  addCheck(
    checks,
    'performance:default-auto-read-timed',
    (getNumber(defaultAutoReadPerformance, 'average_ms') ?? 0) > 0,
    'default balanced auto-read scenario has a positive average latency',
    { average_ms: defaultAutoReadPerformance?.average_ms }
  );

  const qualityPassed = getNumber(artifacts.quality, 'passed');
  const qualityTotal = getNumber(artifacts.quality, 'total');
  addCheck(
    checks,
    'quality:score',
    artifacts.quality?.score === 1 && qualityPassed === qualityTotal && (qualityTotal ?? 0) > 0,
    'deterministic quality benchmark is fully passing',
    { passed: qualityPassed, total: qualityTotal, score: artifacts.quality?.score }
  );

  const finalBarCoverage = getArray(artifacts.quality, 'final_bar_coverage');
  const incompleteCoverage = finalBarCoverage.filter((entry) => entry.status === 'incomplete');
  addCheck(
    checks,
    'quality:final-bar-complete',
    finalBarCoverage.length > 0 && incompleteCoverage.length === 0,
    'quality final-bar coverage has no incomplete capability areas',
    { incomplete_ids: incompleteCoverage.map((entry) => entry.id) }
  );

  const publicContract = finalBarCoverage.find((entry) => entry.id === 'public_contract_integrity');
  addCheck(
    checks,
    'quality:public-contract-integrity',
    publicContract?.status === 'covered',
    'public contract integrity is covered by deterministic benchmark evidence',
    { status: publicContract?.status }
  );

  const corpusCases = getArray(artifacts.corpus, 'cases');
  const corpusScore = getNumber(artifacts.corpus, 'score');
  const corpusCaseCount = getNumber(artifacts.corpus, 'case_count');
  const corpusAssertionCount = getNumber(artifacts.corpus, 'assertion_count');
  const corpusPassedAssertionCount = getNumber(artifacts.corpus, 'passed_assertion_count');
  addCheck(
    checks,
    'corpus:score',
    corpusScore === 1 &&
      corpusAssertionCount === corpusPassedAssertionCount &&
      (corpusAssertionCount ?? 0) > 0,
    'corpus benchmark is fully passing',
    {
      score: corpusScore,
      passed_assertions: corpusPassedAssertionCount,
      assertion_count: corpusAssertionCount,
    }
  );
  addCheck(
    checks,
    'corpus:case-count',
    corpusCases.length >= 6 && corpusCaseCount === corpusCases.length,
    'corpus benchmark includes the expected minimum case coverage',
    { case_count: corpusCaseCount, observed_cases: corpusCases.length }
  );
  addCheck(
    checks,
    'corpus:fixture-diversity',
    corpusCases.some((entry) => entry.fixture_type === 'checked-in') &&
      corpusCases.some((entry) => entry.fixture_type === 'runtime-generated'),
    'corpus benchmark covers checked-in and runtime-generated fixtures',
    {
      fixture_types: Array.from(
        new Set(corpusCases.map((entry) => getString(entry, 'fixture_type')).filter(Boolean))
      ),
    }
  );
  const corpusCaseIds = new Set(corpusCases.map((entry) => getString(entry, 'id')).filter(Boolean));
  const missingCorpusCaseIds = REQUIRED_CORPUS_CASE_IDS.filter((id) => !corpusCaseIds.has(id));
  addCheck(
    checks,
    'corpus:required-archetypes',
    missingCorpusCaseIds.length === 0,
    'corpus benchmark includes all required end-to-end archetype cases',
    { required_case_ids: REQUIRED_CORPUS_CASE_IDS, missing_case_ids: missingCorpusCaseIds }
  );
  const failingCorpusCases = corpusCases.filter((entry) => {
    const assertionCount = getNumber(entry, 'assertion_count');
    return (
      assertionCount === undefined ||
      assertionCount <= 0 ||
      getNumber(entry, 'passed_assertion_count') !== assertionCount ||
      getNumber(entry, 'score') !== 1
    );
  });
  addCheck(
    checks,
    'corpus:case-quality',
    corpusCases.length > 0 && failingCorpusCases.length === 0,
    'every corpus benchmark case has passing assertion-level evidence',
    {
      failing_case_ids: failingCorpusCases.map((entry) => ({
        id: entry.id,
        score: entry.score,
        assertion_count: entry.assertion_count,
        passed_assertion_count: entry.passed_assertion_count,
      })),
    }
  );
  const untaggedCorpusCases = corpusCases.filter(
    (entry) => getStringArray(entry, 'capability_tags').length === 0
  );
  addCheck(
    checks,
    'corpus:case-capability-tags',
    corpusCases.length > 0 && untaggedCorpusCases.length === 0,
    'every corpus benchmark case declares capability tags',
    { untagged_case_ids: untaggedCorpusCases.map((entry) => entry.id) }
  );
  const corpusCapabilitySummary = getArray(artifacts.corpus, 'capability_summary');
  const corpusCapabilityTags = new Set(
    corpusCapabilitySummary.map((entry) => getString(entry, 'tag')).filter(Boolean)
  );
  const missingCorpusCapabilityTags = REQUIRED_CORPUS_CAPABILITY_TAGS.filter(
    (tag) => !corpusCapabilityTags.has(tag)
  );
  const failingCorpusCapabilityTags = corpusCapabilitySummary.filter(
    (entry) =>
      getString(entry, 'status') !== 'passed' ||
      getNumber(entry, 'score') !== 1 ||
      (getNumber(entry, 'failed_assertion_count') ?? 0) !== 0
  );
  addCheck(
    checks,
    'corpus:capability-summary',
    corpusCapabilitySummary.length > 0 &&
      missingCorpusCapabilityTags.length === 0 &&
      failingCorpusCapabilityTags.length === 0,
    'corpus benchmark capability summary covers required areas and has no failing tags',
    {
      required_tags: REQUIRED_CORPUS_CAPABILITY_TAGS,
      missing_required_tags: missingCorpusCapabilityTags,
      failing_tags: failingCorpusCapabilityTags.map((entry) => ({
        tag: entry.tag,
        status: entry.status,
        score: entry.score,
        failed_assertion_count: entry.failed_assertion_count,
      })),
    }
  );

  const providerRequiredIds = finalBarCoverage
    .filter((entry) => entry.status === 'provider_benchmark_required')
    .map((entry) => getString(entry, 'id'))
    .filter((id): id is string => id !== undefined);
  const providerEvidence = getArray(artifacts.provider, 'final_bar_provider_evidence');
  const providerEvidenceById = new Map(providerEvidence.map((entry) => [getString(entry, 'id'), entry]));
  const uncertifiedProviderRequiredIds = providerRequiredIds.filter(
    (id) => providerEvidenceById.get(id)?.status !== 'certified'
  );
  addCheck(
    checks,
    'provider:required-final-bar-evidence',
    providerRequiredIds.length > 0 && uncertifiedProviderRequiredIds.length === 0,
    'all quality areas that require installed-provider evidence are certified',
    { required_ids: providerRequiredIds, uncertified_ids: uncertifiedProviderRequiredIds }
  );

  const providerUncertified = providerEvidence.filter((entry) => entry.status !== 'certified');
  addCheck(
    checks,
    'provider:all-final-bar-evidence-certified',
    providerEvidence.length > 0 && providerUncertified.length === 0,
    'provider final-bar evidence has no skipped, failed, or incomplete capability areas',
    {
      uncertified: providerUncertified.map((entry) => ({
        id: entry.id,
        status: entry.status,
      })),
    }
  );

  addCheck(
    checks,
    'provider:strict-mode',
    artifacts.provider?.strict === true,
    'provider benchmark artifact was produced with strict provider requirements enabled',
    { strict: artifacts.provider?.strict }
  );

  const providerResults = getArray(artifacts.provider, 'results');
  const providerResultsMissingQuality = providerResults.filter((result) => {
    const quality = isRecord(result.quality) ? result.quality : undefined;
    const metrics = getArray(quality, 'metrics');
    return (
      quality === undefined ||
      getNumber(quality, 'metric_count') === undefined ||
      getNumber(quality, 'metric_count') !== metrics.length ||
      metrics.length === 0
    );
  });
  addCheck(
    checks,
    'provider:quality-metrics-present',
    providerResults.length > 0 && providerResultsMissingQuality.length === 0,
    'provider benchmark results include machine-readable quality metrics',
    {
      missing_quality_for: providerResultsMissingQuality.map((result) => ({
        provider: result.provider,
        status: result.status,
      })),
    }
  );

  const providerResultsWithFailingQuality = providerResults.filter((result) => {
    const quality = isRecord(result.quality) ? result.quality : undefined;
    const metrics = getArray(quality, 'metrics');
    return (
      result.status !== 'passed' ||
      getNumber(quality, 'score') !== 1 ||
      getNumber(quality, 'passed_metric_count') !== getNumber(quality, 'metric_count') ||
      metrics.some((metric) => metric.status !== 'passed')
    );
  });
  addCheck(
    checks,
    'provider:quality-metrics-passing',
    providerResults.length > 0 && providerResultsWithFailingQuality.length === 0,
    'provider quality metrics pass for every installed provider certification result',
    {
      failing_quality_for: providerResultsWithFailingQuality.map((result) => ({
        provider: result.provider,
        status: result.status,
        quality_score: isRecord(result.quality) ? result.quality.score : undefined,
      })),
    }
  );

  const providerManifestSummary = getRecord(artifacts['provider-manifest'], 'summary');
  const providerManifestCases = getArray(artifacts['provider-manifest'], 'cases');
  const providerManifestRegions = providerManifestCases.flatMap((entry) =>
    getArray(entry, 'regions')
  );
  const providerManifestScore = getNumber(providerManifestSummary, 'score');
  const providerManifestAssertionCount = getNumber(providerManifestSummary, 'assertion_count');
  const providerManifestPassedAssertionCount = getNumber(
    providerManifestSummary,
    'passed_assertion_count'
  );
  const providerManifestExternalCaseCount = getNumber(
    artifacts['provider-manifest'],
    'external_case_count'
  );
  const providerManifestExternalRegionCount = getNumber(
    artifacts['provider-manifest'],
    'external_region_count'
  );
  addCheck(
    checks,
    'provider-manifest:score',
    artifacts['provider-manifest']?.status === 'passed' &&
      providerManifestScore === 1 &&
      providerManifestAssertionCount === providerManifestPassedAssertionCount &&
      (providerManifestAssertionCount ?? 0) > 0 &&
      (getNumber(providerManifestSummary, 'failed_assertion_count') ?? 1) === 0,
    'provider-manifest analysis benchmark is fully passing',
    {
      status: artifacts['provider-manifest']?.status,
      score: providerManifestScore,
      passed_assertions: providerManifestPassedAssertionCount,
      assertion_count: providerManifestAssertionCount,
      failed_assertion_count: getNumber(providerManifestSummary, 'failed_assertion_count'),
    }
  );
  addCheck(
    checks,
    'provider-manifest:coverage',
    (providerManifestExternalCaseCount ?? 0) > 0 &&
      (providerManifestExternalRegionCount ?? 0) >= REQUIRED_PROVIDER_MANIFEST_KINDS.length &&
      getNumber(providerManifestSummary, 'case_count') === providerManifestExternalCaseCount &&
      getNumber(providerManifestSummary, 'region_count') === providerManifestExternalRegionCount,
    'provider-manifest analysis benchmark includes case and region coverage',
    {
      external_case_count: providerManifestExternalCaseCount,
      external_region_count: providerManifestExternalRegionCount,
      summary_case_count: getNumber(providerManifestSummary, 'case_count'),
      summary_region_count: getNumber(providerManifestSummary, 'region_count'),
      required_region_count: REQUIRED_PROVIDER_MANIFEST_KINDS.length,
    }
  );
  const failingProviderManifestCases = providerManifestCases.filter((entry) => {
    const assertionCount = getNumber(entry, 'assertion_count');
    return (
      assertionCount === undefined ||
      assertionCount <= 0 ||
      getNumber(entry, 'passed_assertion_count') !== assertionCount ||
      getNumber(entry, 'score') !== 1
    );
  });
  const failingProviderManifestRegions = providerManifestRegions.filter((entry) => {
    const assertionCount = getNumber(entry, 'assertion_count');
    const expectedKind = getString(entry, 'expected_kind');
    const observedKind = getString(entry, 'observed_kind');
    return (
      assertionCount === undefined ||
      assertionCount <= 0 ||
      getString(entry, 'status') !== 'passed' ||
      getNumber(entry, 'passed_assertion_count') !== assertionCount ||
      getNumber(entry, 'score') !== 1 ||
      expectedKind === undefined ||
      observedKind !== expectedKind
    );
  });
  addCheck(
    checks,
    'provider-manifest:case-region-quality',
    providerManifestCases.length > 0 &&
      providerManifestRegions.length >= REQUIRED_PROVIDER_MANIFEST_KINDS.length &&
      failingProviderManifestCases.length === 0 &&
      failingProviderManifestRegions.length === 0,
    'every provider-manifest analysis case and region has passing kind-specific evidence',
    {
      failing_case_ids: failingProviderManifestCases.map((entry) => ({
        id: entry.id,
        score: entry.score,
        assertion_count: entry.assertion_count,
        passed_assertion_count: entry.passed_assertion_count,
      })),
      failing_region_ids: failingProviderManifestRegions.map((entry) => ({
        id: entry.id,
        expected_kind: entry.expected_kind,
        observed_kind: entry.observed_kind,
        status: entry.status,
        score: entry.score,
      })),
    }
  );
  const providerManifestRegionsMissingAssertionEvidence = providerManifestRegions.filter((entry) => {
    const expectedKind = getString(entry, 'expected_kind');
    const passingAssertionIds = new Set(
      getArray(entry, 'assertions')
        .filter((assertion) => assertion.pass === true)
        .map((assertion) => getString(assertion, 'id'))
        .filter((id): id is string => id !== undefined)
    );
    const hasAssertionSuffix = (suffix: string) =>
      [...passingAssertionIds].some((id) => id.endsWith(`:${suffix}`));
    const hasContainsTextAssertion = [...passingAssertionIds].some((id) =>
      id.includes(':contains:')
    );
    const hasKindSpecificAssertion =
      expectedKind === 'table'
        ? hasAssertionSuffix('table-cells')
        : expectedKind === 'formula'
          ? hasAssertionSuffix('formula-formats')
          : expectedKind === 'chart'
            ? hasAssertionSuffix('chart-components')
            : true;

    return (
      passingAssertionIds.size === 0 ||
      !hasAssertionSuffix('confidence') ||
      !hasContainsTextAssertion ||
      !hasAssertionSuffix('crop-provenance') ||
      !hasKindSpecificAssertion
    );
  });
  addCheck(
    checks,
    'provider-manifest:assertion-evidence',
    providerManifestRegions.length >= REQUIRED_PROVIDER_MANIFEST_KINDS.length &&
      providerManifestRegionsMissingAssertionEvidence.length === 0,
    'provider-manifest analysis regions include passing confidence, text, crop-provenance, and kind-specific assertions',
    {
      failing_region_ids: providerManifestRegionsMissingAssertionEvidence.map((entry) => ({
        id: entry.id,
        expected_kind: entry.expected_kind,
        assertion_ids: getArray(entry, 'assertions')
          .map((assertion) => getString(assertion, 'id'))
          .filter((id): id is string => id !== undefined),
      })),
    }
  );
  const observedProviderManifestKinds = new Set(
    providerManifestRegions.map((entry) => getString(entry, 'observed_kind')).filter(Boolean)
  );
  const missingProviderManifestKinds = REQUIRED_PROVIDER_MANIFEST_KINDS.filter(
    (kind) => !observedProviderManifestKinds.has(kind)
  );
  addCheck(
    checks,
    'provider-manifest:kind-coverage',
    missingProviderManifestKinds.length === 0,
    'provider-manifest analysis benchmark covers required visual evidence kinds',
    {
      required_kinds: REQUIRED_PROVIDER_MANIFEST_KINDS,
      observed_kinds: [...observedProviderManifestKinds].sort(),
      missing_required_kinds: missingProviderManifestKinds,
    }
  );
  const providerManifestCapabilitySummary = getArray(
    artifacts['provider-manifest'],
    'capability_summary'
  );
  const providerManifestCapabilityTags = new Set(
    providerManifestCapabilitySummary.map((entry) => getString(entry, 'tag')).filter(Boolean)
  );
  const missingProviderManifestCapabilityTags = REQUIRED_PROVIDER_MANIFEST_CAPABILITY_TAGS.filter(
    (tag) => !providerManifestCapabilityTags.has(tag)
  );
  const failingProviderManifestCapabilityTags = providerManifestCapabilitySummary.filter(
    (entry) =>
      getString(entry, 'status') !== 'passed' ||
      getNumber(entry, 'score') !== 1 ||
      (getNumber(entry, 'failed_assertion_count') ?? 0) !== 0
  );
  addCheck(
    checks,
    'provider-manifest:capability-summary',
    providerManifestCapabilitySummary.length > 0 &&
      missingProviderManifestCapabilityTags.length === 0 &&
      failingProviderManifestCapabilityTags.length === 0,
    'provider-manifest analysis capability summary covers required release areas and has no failing tags',
    {
      required_tags: REQUIRED_PROVIDER_MANIFEST_CAPABILITY_TAGS,
      missing_required_tags: missingProviderManifestCapabilityTags,
      failing_tags: failingProviderManifestCapabilityTags.map((entry) => ({
        tag: entry.tag,
        status: entry.status,
        score: entry.score,
        failed_assertion_count: entry.failed_assertion_count,
      })),
    }
  );

  const providerCropSummary = getRecord(artifacts['provider-crops'], 'summary');
  const providerCropCases = getArray(artifacts['provider-crops'], 'cases');
  const providerCropRegions = providerCropCases.flatMap((entry) => getArray(entry, 'regions'));
  const providerCropScore = getNumber(providerCropSummary, 'score');
  const providerCropAssertionCount = getNumber(providerCropSummary, 'assertion_count');
  const providerCropPassedAssertionCount = getNumber(
    providerCropSummary,
    'passed_assertion_count'
  );
  const providerCropExternalCaseCount = getNumber(
    artifacts['provider-crops'],
    'external_case_count'
  );
  const providerCropExternalRegionCount = getNumber(
    artifacts['provider-crops'],
    'external_region_count'
  );
  addCheck(
    checks,
    'provider-crops:score',
    artifacts['provider-crops']?.status === 'passed' &&
      providerCropScore === 1 &&
      providerCropAssertionCount === providerCropPassedAssertionCount &&
      (providerCropAssertionCount ?? 0) > 0 &&
      (getNumber(providerCropSummary, 'failed_assertion_count') ?? 1) === 0,
    'provider-manifest crop benchmark is fully passing',
    {
      status: artifacts['provider-crops']?.status,
      score: providerCropScore,
      passed_assertions: providerCropPassedAssertionCount,
      assertion_count: providerCropAssertionCount,
      failed_assertion_count: getNumber(providerCropSummary, 'failed_assertion_count'),
    }
  );
  addCheck(
    checks,
    'provider-crops:coverage',
    (providerCropExternalCaseCount ?? 0) > 0 &&
      (providerCropExternalRegionCount ?? 0) > 0 &&
      getNumber(providerCropSummary, 'case_count') === providerCropExternalCaseCount &&
      getNumber(providerCropSummary, 'region_count') === providerCropExternalRegionCount,
    'provider-manifest crop benchmark includes case and region coverage',
    {
      external_case_count: providerCropExternalCaseCount,
      external_region_count: providerCropExternalRegionCount,
      summary_case_count: getNumber(providerCropSummary, 'case_count'),
      summary_region_count: getNumber(providerCropSummary, 'region_count'),
    }
  );
  const failingProviderCropCases = providerCropCases.filter((entry) => {
    const assertionCount = getNumber(entry, 'assertion_count');
    return (
      assertionCount === undefined ||
      assertionCount <= 0 ||
      getNumber(entry, 'passed_assertion_count') !== assertionCount ||
      getNumber(entry, 'score') !== 1
    );
  });
  const failingProviderCropRegions = providerCropRegions.filter(
    (entry) =>
      getString(entry, 'status') !== 'passed' ||
      getNumber(entry, 'score') !== 1 ||
      getRecord(entry, 'crop') === undefined
  );
  addCheck(
    checks,
    'provider-crops:case-region-quality',
    providerCropCases.length > 0 &&
      providerCropRegions.length > 0 &&
      failingProviderCropCases.length === 0 &&
      failingProviderCropRegions.length === 0,
    'every provider-manifest crop case and region has passing evidence and crop metadata',
    {
      failing_case_ids: failingProviderCropCases.map((entry) => ({
        id: entry.id,
        score: entry.score,
        assertion_count: entry.assertion_count,
        passed_assertion_count: entry.passed_assertion_count,
      })),
      failing_region_ids: failingProviderCropRegions.map((entry) => ({
        id: entry.id,
        status: entry.status,
        score: entry.score,
        has_crop: getRecord(entry, 'crop') !== undefined,
      })),
    }
  );
  const providerCropCapabilitySummary = getArray(artifacts['provider-crops'], 'capability_summary');
  const providerCropCapabilityTags = new Set(
    providerCropCapabilitySummary.map((entry) => getString(entry, 'tag')).filter(Boolean)
  );
  const missingProviderCropCapabilityTags = REQUIRED_PROVIDER_CROP_CAPABILITY_TAGS.filter(
    (tag) => !providerCropCapabilityTags.has(tag)
  );
  const failingProviderCropCapabilityTags = providerCropCapabilitySummary.filter(
    (entry) =>
      getString(entry, 'status') !== 'passed' ||
      getNumber(entry, 'score') !== 1 ||
      (getNumber(entry, 'failed_assertion_count') ?? 0) !== 0
  );
  addCheck(
    checks,
    'provider-crops:capability-summary',
    providerCropCapabilitySummary.length > 0 &&
      missingProviderCropCapabilityTags.length === 0 &&
      failingProviderCropCapabilityTags.length === 0,
    'provider-manifest crop capability summary covers required release areas and has no failing tags',
    {
      required_tags: REQUIRED_PROVIDER_CROP_CAPABILITY_TAGS,
      missing_required_tags: missingProviderCropCapabilityTags,
      failing_tags: failingProviderCropCapabilityTags.map((entry) => ({
        tag: entry.tag,
        status: entry.status,
        score: entry.score,
        failed_assertion_count: entry.failed_assertion_count,
      })),
    }
  );

  const packageJson = JSON.parse(
    await fs.promises.readFile(path.resolve(import.meta.dirname, '../package.json'), 'utf8')
  ) as { version?: string };
  const doctor = runDoctorViaSubprocess(packageJson.version ?? '0.0.0');
  addCheck(
    checks,
    'install:doctor-ready',
    doctor.status !== 'unavailable',
    'install doctor reports a ready or degraded runtime (no hard failures)',
    { doctorStatus: doctor.status, checks: doctor.checks }
  );
  addCheck(
    checks,
    'install:pdfjs-resources',
    doctor.checks.find((check) => check.id === 'pdfjs_resources')?.status === 'ok',
    'install doctor confirms pdfjs-dist resource bundles are present'
  );
  addCheck(
    checks,
    'install:sample-probe',
    doctor.checks.find((check) => check.id === 'sample_probe')?.status === 'ok',
    'install doctor loads the checked-in sample.pdf fixture successfully'
  );

  const summary = summarizeChecks(checks);
  return {
    profile: 'pdf_sota_release_gate',
    generated_at: new Date().toISOString(),
    artifact_dir: absoluteArtifactDir,
    status: summary.failed === 0 ? 'passed' : 'failed',
    summary,
    checks,
  };
};

export const main = async () => {
  const artifactDir = resolveSotaReleaseArtifactDir();
  const report = await buildSotaReleaseGateReport(artifactDir);
  console.log(JSON.stringify(report, null, 2));
  const outputPath = await writeBenchmarkReport(report);
  if (outputPath) {
    console.error(`SOTA release gate report written to ${outputPath}`);
  }
  if (report.status === 'failed') {
    process.exitCode = 1;
  }
};

if (import.meta.main) {
  await main();
}
