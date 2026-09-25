import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);

type SmokeStatus = 'passed' | 'failed';

interface PackageSmokeCheck {
  id: string;
  status: SmokeStatus;
  message: string;
  evidence?: Record<string, unknown> | undefined;
}

export interface PackageSmokeReport {
  profile: 'pdf_package_smoke';
  generated_at: string;
  status: SmokeStatus;
  tarball_file?: string | undefined;
  summary: {
    total: number;
    passed: number;
    failed: number;
  };
  checks: PackageSmokeCheck[];
}

type JsonRecord = Record<string, unknown>;

const REQUIRED_PUBLIC_CORPUS_CAPABILITY_TAGS = [
  'accessibility_guidance',
  'chart_evidence',
  'document_map',
  'fillable_form',
  'formula_text',
  'government_newsletter',
  'image_plus_text',
  'legacy_scan',
  'official_form',
  'public_domain_text',
  'research_paper',
  'selectable_text',
  'statistical_report',
  'table_evidence',
  'technical_report',
  'text_layer',
  'visual_text',
] as const;

const REQUIRED_PUBLIC_PROVIDER_CAPABILITY_TAGS = [
  'accessibility_diagram',
  'chart_extraction',
  'crop_provenance',
  'diagram_context',
  'figure_description',
  'full_page_crop',
  'formula_recognition',
  'image_description',
  'image_plus_text',
  'layout_diagram',
  'legacy_scan',
  'scanned_page_triage',
  'table_recognition',
  'visual_text',
] as const;

const REQUIRED_PUBLIC_PROVIDER_EXPECTED_KINDS = [
  'chart',
  'diagram',
  'figure',
  'formula',
  'image',
  'table',
] as const;

const addCheck = (
  checks: PackageSmokeCheck[],
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

const summarizeChecks = (checks: PackageSmokeCheck[]): PackageSmokeReport['summary'] => ({
  total: checks.length,
  passed: checks.filter((check) => check.status === 'passed').length,
  failed: checks.filter((check) => check.status === 'failed').length,
});

const isRecord = (value: unknown): value is JsonRecord =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const isNonEmptyString = (value: unknown): value is string =>
  typeof value === 'string' && value.trim().length > 0;

const positiveInteger = (value: unknown): number | undefined =>
  typeof value === 'number' && Number.isInteger(value) && value > 0 ? value : undefined;

const nonNegativeNumber = (value: unknown): number | undefined =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined;

const normalizedConfidence = (value: unknown): number | undefined =>
  typeof value === 'number' && Number.isFinite(value) && value > 0 && value <= 1 ? value : undefined;

const getRecord = (value: unknown): JsonRecord | undefined => (isRecord(value) ? value : undefined);

const expectedKind = (entry: JsonRecord): string | undefined => {
  const kind = getRecord(entry.expected)?.kind;
  return typeof kind === 'string' && kind.trim().length > 0 ? kind.trim().toLowerCase() : undefined;
};

const normalizedTags = (value: unknown): string[] => {
  if (!Array.isArray(value)) return [];
  const tags = value
    .map((entry) =>
      typeof entry === 'string'
        ? entry
            .trim()
            .toLowerCase()
            .replace(/[^a-z0-9._:-]+/gu, '-')
            .replace(/^-+|-+$/gu, '')
        : ''
    )
    .filter((entry) => entry.length > 0);
  return [...new Set(tags)];
};

const missingRequiredTags = (
  tags: Iterable<string>,
  requiredTags: readonly string[]
): string[] => {
  const actual = new Set(tags);
  return requiredTags.filter((tag) => !actual.has(tag));
};

const missingRequiredKinds = (
  kinds: Iterable<string>,
  requiredKinds: readonly string[]
): string[] => {
  const actual = new Set(kinds);
  return requiredKinds.filter((kind) => !actual.has(kind));
};

const readJson = async (filePath: string): Promise<JsonRecord | undefined> => {
  try {
    const raw = fs.readFileSync(filePath, 'utf8');
    const parsed = JSON.parse(raw) as unknown;
    return isRecord(parsed) ? parsed : undefined;
  } catch {
    return undefined;
  }
};

const hasExpectedTextAssertions = (entry: JsonRecord): boolean => {
  const expected = getRecord(entry.expected);
  return (
    Array.isArray(expected?.contains_text) &&
    expected.contains_text.some((value) => isNonEmptyString(value))
  );
};

const hasRequiredReadOption = (entry: JsonRecord, key: string): boolean =>
  getRecord(entry.read_pdf_options)?.[key] === true;

const hasValidBoundingBox = (entry: JsonRecord): boolean => {
  const box = getRecord(entry.bounding_box);
  const left = nonNegativeNumber(box?.left);
  const bottom = nonNegativeNumber(box?.bottom);
  const right = nonNegativeNumber(box?.right);
  const top = nonNegativeNumber(box?.top);

  return (
    positiveInteger(entry.page) !== undefined &&
    left !== undefined &&
    bottom !== undefined &&
    right !== undefined &&
    top !== undefined &&
    right > left &&
    top > bottom
  );
};

const fileExists = async (filePath: string): Promise<boolean> => {
  try {
    const stat = fs.statSync(filePath);
    return stat.isFile();
  } catch {
    return false;
  }
};

const readTextPrefix = async (filePath: string, byteLength = 128): Promise<string | undefined> => {
  try {
    const raw = fs.readFileSync(filePath, 'utf8');
    return raw.slice(0, byteLength);
  } catch {
    return undefined;
  }
};

const summarizePublicCorpusManifest = (
  manifest: JsonRecord | undefined
): {
  total_cases: number;
  url_cases_with_metadata: number;
  cases_with_capability_tags: number;
  cases_with_expected_text: number;
  cases_with_expected_page_floor: number;
  cases_with_expected_text_volume: number;
  cases_with_document_map_option: number;
  cases_with_text_layer_option: number;
  capability_tag_count: number;
  capability_tags: string[];
  missing_required_capability_tags: string[];
} => {
  const cases = Array.isArray(manifest?.cases) ? manifest.cases : [];
  const capabilityTags = new Set<string>();
  for (const entry of cases) {
    if (!isRecord(entry)) continue;
    for (const tag of normalizedTags(entry.capability_tags)) {
      capabilityTags.add(tag);
    }
  }
  const urlCasesWithMetadata = cases.filter(
    (entry) =>
      isRecord(entry) &&
      isNonEmptyString(entry.url) &&
      isNonEmptyString(entry.sha256) &&
      isNonEmptyString(entry.source_label) &&
      isNonEmptyString(entry.source_homepage) &&
      isNonEmptyString(entry.source_rights) &&
      isNonEmptyString(entry.source_retrieved_at)
  ).length;

  return {
    total_cases: cases.length,
    url_cases_with_metadata: urlCasesWithMetadata,
    cases_with_capability_tags: cases.filter(
      (entry) => isRecord(entry) && normalizedTags(entry.capability_tags).length > 0
    ).length,
    cases_with_expected_text: cases.filter((entry) => isRecord(entry) && hasExpectedTextAssertions(entry))
      .length,
    cases_with_expected_page_floor: cases.filter(
      (entry) => isRecord(entry) && positiveInteger(getRecord(entry.expected)?.min_pages) !== undefined
    ).length,
    cases_with_expected_text_volume: cases.filter(
      (entry) =>
        isRecord(entry) && positiveInteger(getRecord(entry.expected)?.min_text_chars) !== undefined
    ).length,
    cases_with_document_map_option: cases.filter(
      (entry) => isRecord(entry) && hasRequiredReadOption(entry, 'include_document_map')
    ).length,
    cases_with_text_layer_option: cases.filter(
      (entry) => isRecord(entry) && hasRequiredReadOption(entry, 'include_text_layer')
    ).length,
    capability_tag_count: capabilityTags.size,
    capability_tags: [...capabilityTags].sort(),
    missing_required_capability_tags: missingRequiredTags(
      capabilityTags,
      REQUIRED_PUBLIC_CORPUS_CAPABILITY_TAGS
    ),
  };
};

const summarizePublicProviderManifest = (
  manifest: JsonRecord | undefined
): {
  total_cases: number;
  total_regions: number;
  url_cases_with_metadata: number;
  cases_with_capability_tags: number;
  regions_with_capability_tags: number;
  regions_with_valid_bounding_boxes: number;
  regions_with_expected_kind: number;
  regions_with_expected_text: number;
  regions_with_min_confidence: number;
  capability_tag_count: number;
  capability_tags: string[];
  missing_required_capability_tags: string[];
  expected_kind_count: number;
  expected_kinds: string[];
  missing_required_expected_kinds: string[];
} => {
  const cases = Array.isArray(manifest?.cases) ? manifest.cases : [];
  const regionsByCase = cases.map((entry) =>
    isRecord(entry) && Array.isArray(entry.regions) ? entry.regions.length : 0
  );
  const regions = cases.flatMap((entry) =>
    isRecord(entry) && Array.isArray(entry.regions) ? entry.regions : []
  );
  const capabilityTags = new Set<string>();
  const expectedKinds = new Set<string>();
  for (const entry of cases) {
    if (!isRecord(entry)) continue;
    for (const tag of normalizedTags(entry.capability_tags)) {
      capabilityTags.add(tag);
    }
  }
  for (const entry of regions) {
    if (!isRecord(entry)) continue;
    for (const tag of normalizedTags(entry.capability_tags)) {
      capabilityTags.add(tag);
    }
    const kind = expectedKind(entry);
    if (kind) expectedKinds.add(kind);
  }
  const urlCasesWithMetadata = cases.filter(
    (entry) =>
      isRecord(entry) &&
      isNonEmptyString(entry.url) &&
      isNonEmptyString(entry.sha256) &&
      isNonEmptyString(entry.source_label) &&
      isNonEmptyString(entry.source_homepage) &&
      isNonEmptyString(entry.source_rights) &&
      isNonEmptyString(entry.source_retrieved_at) &&
      Array.isArray(entry.regions) &&
      entry.regions.length > 0
  ).length;

  return {
    total_cases: cases.length,
    total_regions: regionsByCase.reduce((sum, count) => sum + count, 0),
    url_cases_with_metadata: urlCasesWithMetadata,
    cases_with_capability_tags: cases.filter(
      (entry) => isRecord(entry) && normalizedTags(entry.capability_tags).length > 0
    ).length,
    regions_with_capability_tags: regions.filter(
      (entry) => isRecord(entry) && normalizedTags(entry.capability_tags).length > 0
    ).length,
    regions_with_valid_bounding_boxes: regions.filter(
      (entry) => isRecord(entry) && hasValidBoundingBox(entry)
    ).length,
    regions_with_expected_kind: regions.filter((entry) => isRecord(entry) && expectedKind(entry)).length,
    regions_with_expected_text: regions.filter(
      (entry) => isRecord(entry) && hasExpectedTextAssertions(entry)
    ).length,
    regions_with_min_confidence: regions.filter(
      (entry) =>
        isRecord(entry) && normalizedConfidence(getRecord(entry.expected)?.min_confidence) !== undefined
    ).length,
    capability_tag_count: capabilityTags.size,
    capability_tags: [...capabilityTags].sort(),
    missing_required_capability_tags: missingRequiredTags(
      capabilityTags,
      REQUIRED_PUBLIC_PROVIDER_CAPABILITY_TAGS
    ),
    expected_kind_count: expectedKinds.size,
    expected_kinds: [...expectedKinds].sort(),
    missing_required_expected_kinds: missingRequiredKinds(
      expectedKinds,
      REQUIRED_PUBLIC_PROVIDER_EXPECTED_KINDS
    ),
  };
};

export const findPackedTarballPath = async (
  packOutput: string,
  destinationDir: string
): Promise<string | undefined> => {
  const outputPath = packOutput
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .find((line) => line.endsWith('.tgz') && path.isAbsolute(line));

  if (outputPath && (await fileExists(outputPath))) {
    return outputPath;
  }

  const entries = fs.readdirSync(destinationDir);
  const tarballs = entries.filter((entry) => entry.endsWith('.tgz')).sort();
  const fallback = tarballs.at(-1);
  return fallback ? path.join(destinationDir, fallback) : undefined;
};

export const validateExtractedPackage = async (
  packageDir: string
): Promise<PackageSmokeCheck[]> => {
  const checks: PackageSmokeCheck[] = [];
  const packageJsonPath = path.join(packageDir, 'package.json');
  const packageJson = await readJson(packageJsonPath);
  const tsEntryPath = path.join(packageDir, 'dist', 'index.js');
  const pureRustEntryPath = path.join(packageDir, 'dist', 'pure-rust.js');
  const runtimeEntryPath = path.join(packageDir, 'dist', 'runtime-entry.js');
  const publicCorpusManifestPath = path.join(packageDir, 'corpus', 'public-url-corpus.json');
  const publicProviderManifestPath = path.join(
    packageDir,
    'corpus',
    'public-provider-accuracy.json'
  );
  const publicCorpusManifest = await readJson(publicCorpusManifestPath);
  const publicProviderManifest = await readJson(publicProviderManifestPath);
  const publicCorpusSummary = summarizePublicCorpusManifest(publicCorpusManifest);
  const publicProviderSummary = summarizePublicProviderManifest(publicProviderManifest);
  const bin = isRecord(packageJson?.bin) ? packageJson.bin : undefined;
  const exportsField = isRecord(packageJson?.exports) ? packageJson.exports : undefined;

  addCheck(checks, 'package-json:present', packageJson !== undefined, 'package.json exists and is valid JSON', {
    path: 'package/package.json',
    name: packageJson?.name,
    version: packageJson?.version,
  });
  const productionDependencies = isRecord(packageJson?.dependencies) ? packageJson.dependencies : {};
  const productionDependencyNames = Object.keys(productionDependencies);
  const bannedProductionDependencies = productionDependencyNames.filter((name) =>
    ['pdfjs-dist', '@modelcontextprotocol/sdk', 'pngjs', 'zod'].includes(name) ||
    name.toLowerCase().includes('pdfjs')
  );
  addCheck(
    checks,
    'package-json:empty-production-dependencies',
    productionDependencyNames.length === 0,
    'published package declares no production npm dependencies (sole-Rust launcher)',
    { productionDependencyNames }
  );
  addCheck(
    checks,
    'package-json:no-banned-production-dependencies',
    bannedProductionDependencies.length === 0,
    'published package does not declare pdfjs/MCP-TS-sdk/pngjs/zod production dependencies',
    { bannedProductionDependencies }
  );
  addCheck(
    checks,
    'runtime:ts-entry-absent',
    !(await fileExists(tsEntryPath)),
    'published package does not ship TypeScript MCP entry dist/index.js',
    { path: 'package/dist/index.js' }
  );
  addCheck(
    checks,
    'runtime:default-bin-contract',
    bin?.anymd === './dist/runtime-entry.js' &&
      exportsField?.['.'] === './dist/runtime-entry.js' &&
      (await fileExists(runtimeEntryPath)),
    'package bin/exports are sole-Rust runtime-entry.js only',
    { bin: bin?.anymd, exports: exportsField?.['.'] }
  );
  addCheck(
    checks,
    'runtime:typescript-export-absent',
    exportsField?.['./typescript'] === undefined,
    'package does not export ./typescript production fallback',
    { export: exportsField?.['./typescript'] }
  );
  addCheck(
    checks,
    'runtime:pure-rust-export-contract',
    exportsField?.['./pure-rust'] === './dist/pure-rust.js' && (await fileExists(pureRustEntryPath)),
    'package exposes thin pure-Rust spawn helper at ./pure-rust',
    {
      export: exportsField?.['./pure-rust'],
      path: 'package/dist/pure-rust.js',
      productTruth: { soleRustProduction: true, typescriptProductionShipped: false },
    }
  );
  addCheck(
    checks,
    'corpus:public-url-manifest',
    await fileExists(publicCorpusManifestPath),
    'published package includes the opt-in public URL corpus manifest',
    { path: 'package/corpus/public-url-corpus.json' }
  );
  addCheck(
    checks,
    'corpus:public-url-manifest-shape',
    publicCorpusSummary.total_cases > 0 &&
      publicCorpusSummary.total_cases === publicCorpusSummary.url_cases_with_metadata &&
      publicCorpusSummary.total_cases === publicCorpusSummary.cases_with_capability_tags &&
      publicCorpusSummary.total_cases === publicCorpusSummary.cases_with_expected_text &&
      publicCorpusSummary.total_cases === publicCorpusSummary.cases_with_expected_page_floor &&
      publicCorpusSummary.total_cases === publicCorpusSummary.cases_with_expected_text_volume &&
      publicCorpusSummary.total_cases === publicCorpusSummary.cases_with_document_map_option &&
      publicCorpusSummary.total_cases === publicCorpusSummary.cases_with_text_layer_option &&
      publicCorpusSummary.missing_required_capability_tags.length === 0,
    'public URL corpus manifest contains URL cases, source metadata, SHA256 values, expected assertions, read options, and required capability tags',
    publicCorpusSummary
  );
  addCheck(
    checks,
    'corpus:public-provider-manifest',
    await fileExists(publicProviderManifestPath),
    'published package includes the opt-in public provider accuracy manifest',
    { path: 'package/corpus/public-provider-accuracy.json' }
  );
  addCheck(
    checks,
    'corpus:public-provider-manifest-shape',
    publicProviderSummary.total_cases > 0 &&
      publicProviderSummary.total_regions > 0 &&
      publicProviderSummary.total_cases === publicProviderSummary.url_cases_with_metadata &&
      publicProviderSummary.total_cases === publicProviderSummary.cases_with_capability_tags &&
      publicProviderSummary.total_regions === publicProviderSummary.regions_with_capability_tags &&
      publicProviderSummary.total_regions === publicProviderSummary.regions_with_valid_bounding_boxes &&
      publicProviderSummary.total_regions === publicProviderSummary.regions_with_expected_kind &&
      publicProviderSummary.total_regions === publicProviderSummary.regions_with_expected_text &&
      publicProviderSummary.total_regions === publicProviderSummary.regions_with_min_confidence &&
      publicProviderSummary.missing_required_capability_tags.length === 0 &&
      publicProviderSummary.missing_required_expected_kinds.length === 0,
    'public provider manifest contains URL cases, source metadata, SHA256 values, scored regions, expected kinds, expected assertions, and required capability tags',
    publicProviderSummary
  );
  const files = Array.isArray(packageJson?.files) ? packageJson.files : [];
  addCheck(
    checks,
    'package-json:files',
    files.includes('dist/runtime-entry.js') &&
      files.includes('dist/pure-rust.js') &&
      (files.includes('corpus/') || files.includes('corpus')) &&
      !files.includes('dist/') &&
      !files.includes('dist'),
    'package files allowlist ships sole-Rust launcher artifacts and corpus only',
    { files }
  );

  return checks;
};

const listTarballEntries = async (tarballPath: string): Promise<string[]> => {
  const { stdout } = await execFileAsync('tar', ['-tzf', tarballPath], {
    maxBuffer: 10 * 1024 * 1024,
  });
  return stdout
    .split(/\r?\n/u)
    .map((entry) => entry.trim())
    .filter(Boolean);
};

export const buildPackageSmokeReport = async (cwd = process.cwd()): Promise<PackageSmokeReport> => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pdf-reader-package-smoke-'));
  const checks: PackageSmokeCheck[] = [];
  let tarballPath: string | undefined;

  try {
    const { stdout } = await execFileAsync('bun', ['pm', 'pack', '--destination', tempDir], {
      cwd,
      maxBuffer: 10 * 1024 * 1024,
    });
    tarballPath = await findPackedTarballPath(stdout, tempDir);
    addCheck(checks, 'pack:tarball-created', tarballPath !== undefined, 'bun pack produced a tarball', {
      tarball_file: tarballPath ? path.basename(tarballPath) : undefined,
    });

    if (!tarballPath) {
      const summary = summarizeChecks(checks);
      return {
        profile: 'pdf_package_smoke',
        generated_at: new Date().toISOString(),
        status: 'failed',
        summary,
        checks,
      };
    }

    const tarballEntries = await listTarballEntries(tarballPath);
    addCheck(
      checks,
      'tarball:ts-entry-absent',
      !tarballEntries.includes('package/dist/index.js') &&
        !tarballEntries.includes('package/dist/legacy-engine-runtime.js') &&
        !tarballEntries.some((e) => e.includes('pdf.worker')),
      'tarball excludes TypeScript MCP entry, legacy runtime, and PDF.js workers',
      { entries: tarballEntries.filter((e) => e.includes('dist/')).slice(0, 40) }
    );
    addCheck(
      checks,
      'tarball:runtime-entry',
      tarballEntries.includes('package/dist/runtime-entry.js'),
      'tarball includes sole-Rust runtime-entry.js launcher'
    );
    addCheck(
      checks,
      'tarball:pure-rust-entry',
      tarballEntries.includes('package/dist/pure-rust.js'),
      'tarball includes thin pure-rust spawn helper export'
    );
    addCheck(
      checks,
      'tarball:package-json',
      tarballEntries.includes('package/package.json'),
      'tarball includes package/package.json'
    );
    addCheck(
      checks,
      'tarball:readme-license',
      tarballEntries.includes('package/README.md') && tarballEntries.includes('package/LICENSE'),
      'tarball includes README and LICENSE'
    );
    addCheck(
      checks,
      'tarball:public-url-corpus',
      tarballEntries.includes('package/corpus/public-url-corpus.json'),
      'tarball includes public URL corpus manifest'
    );
    addCheck(
      checks,
      'tarball:public-provider-manifest',
      tarballEntries.includes('package/corpus/public-provider-accuracy.json'),
      'tarball includes public provider accuracy manifest'
    );
    const extractDir = path.join(tempDir, 'extract');
    fs.mkdirSync(extractDir, { recursive: true });
    await execFileAsync('tar', ['-xzf', tarballPath, '-C', extractDir], {
      maxBuffer: 10 * 1024 * 1024,
    });
    checks.push(...(await validateExtractedPackage(path.join(extractDir, 'package'))));
  } catch (error) {
    addCheck(checks, 'pack:command', false, 'package smoke command failed', {
      error: error instanceof Error ? error.message : String(error),
    });
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }

  const summary = summarizeChecks(checks);
  return {
    profile: 'pdf_package_smoke',
    generated_at: new Date().toISOString(),
    status: summary.failed === 0 ? 'passed' : 'failed',
    tarball_file: tarballPath ? path.basename(tarballPath) : undefined,
    summary,
    checks,
  };
};

export const main = async () => {
  const report = await buildPackageSmokeReport();
  console.log(JSON.stringify(report, null, 2));
  if (report.status === 'failed') {
    process.exitCode = 1;
  }
};

if (import.meta.main) {
  await main();
}
