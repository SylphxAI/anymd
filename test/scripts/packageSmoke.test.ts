import { describe, expect, test } from 'bun:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { findPackedTarballPath, validateExtractedPackage } from '../../scripts/package-smoke.js';

type JsonValue = string | number | boolean | null | JsonValue[] | { [key: string]: JsonValue };

const withTempDir = async <T>(run: (tempDir: string) => Promise<T>): Promise<T> => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pdf-reader-package-smoke-test-'));
  try {
    return await run(tempDir);
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
};

const writeJson = (filePath: string, value: JsonValue) => {
  fs.writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
};

const writeExtractedPackage = (packageDir: string, includeRuntime = true) => {
  fs.mkdirSync(path.join(packageDir, 'corpus'), { recursive: true });
  fs.mkdirSync(path.join(packageDir, 'dist'), { recursive: true });
  if (includeRuntime) {
    // sole-Rust package must not ship dist/index.js
    fs.writeFileSync(path.join(packageDir, 'dist', 'pure-rust.js'), 'export {};\n', 'utf8');
    fs.writeFileSync(path.join(packageDir, 'dist', 'runtime-entry.js'), 'export {};\n', 'utf8');
  }
  writeJson(path.join(packageDir, 'package.json'), {
    name: '@sylphx/anymd',
    version: '4.0.1',
    bin: {
      anymd: './dist/runtime-entry.js',
    },
    exports: {
      '.': './dist/runtime-entry.js',
      './pure-rust': './dist/pure-rust.js',
    },
    files: ['dist/runtime-entry.js', 'dist/pure-rust.js', 'corpus/', 'README.md', 'LICENSE'],
  });
  writeJson(path.join(packageDir, 'corpus', 'public-url-corpus.json'), {
    cases: [
      {
        id: 'public-smoke',
        url: 'https://example.com/public-smoke.pdf',
        sha256: 'a'.repeat(64),
        source_label: 'public smoke fixture',
        source_homepage: 'https://example.com/',
        source_rights: 'test fixture',
        source_retrieved_at: '2026-06-21',
        capability_tags: [
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
        ],
        expected: {
          min_pages: 1,
          min_text_chars: 100,
          contains_text: ['public smoke'],
        },
        read_pdf_options: {
          include_full_text: true,
          include_text_layer: true,
          include_document_map: true,
        },
      },
    ],
  });
  writeJson(path.join(packageDir, 'corpus', 'public-provider-accuracy.json'), {
    cases: [
      {
        id: 'provider-smoke',
        url: 'https://example.com/provider-smoke.pdf',
        sha256: 'b'.repeat(64),
        source_label: 'provider smoke fixture',
        source_homepage: 'https://example.com/',
        source_rights: 'test fixture',
        source_retrieved_at: '2026-06-21',
        capability_tags: [
          'accessibility_diagram',
          'chart_extraction',
          'figure_description',
          'formula_recognition',
          'image_plus_text',
          'legacy_scan',
          'table_recognition',
          'visual_text',
        ],
        regions: [
          {
            id: 'provider-diagram',
            page: 1,
            bounding_box: { left: 0, bottom: 0, right: 100, top: 100 },
            capability_tags: [
              'crop_provenance',
              'diagram_context',
              'full_page_crop',
              'layout_diagram',
              'scanned_page_triage',
            ],
            expected: { kind: 'diagram', contains_text: ['provider'], min_confidence: 0.2 },
          },
          {
            id: 'provider-chart',
            page: 1,
            bounding_box: { left: 0, bottom: 100, right: 100, top: 200 },
            capability_tags: ['chart_extraction', 'crop_provenance'],
            expected: { kind: 'chart', contains_text: ['chart'], min_confidence: 0.2 },
          },
          {
            id: 'provider-figure',
            page: 1,
            bounding_box: { left: 0, bottom: 200, right: 100, top: 300 },
            capability_tags: ['crop_provenance', 'figure_description'],
            expected: { kind: 'figure', contains_text: ['figure'], min_confidence: 0.2 },
          },
          {
            id: 'provider-formula',
            page: 1,
            bounding_box: { left: 0, bottom: 300, right: 100, top: 400 },
            capability_tags: ['crop_provenance', 'formula_recognition'],
            expected: { kind: 'formula', contains_text: ['formula'], min_confidence: 0.2 },
          },
          {
            id: 'provider-image',
            page: 1,
            bounding_box: { left: 0, bottom: 400, right: 100, top: 500 },
            capability_tags: ['crop_provenance', 'image_description'],
            expected: { kind: 'image', contains_text: ['image'], min_confidence: 0.2 },
          },
          {
            id: 'provider-table',
            page: 1,
            bounding_box: { left: 0, bottom: 500, right: 100, top: 600 },
            capability_tags: ['crop_provenance', 'table_recognition'],
            expected: { kind: 'table', contains_text: ['table'], min_confidence: 0.2 },
          },
        ],
      },
    ],
  });
};

describe('package smoke', () => {
  test('validates the packed package runtime contract', async () => {
    await withTempDir(async (tempDir) => {
      writeExtractedPackage(tempDir);

      const checks = await validateExtractedPackage(tempDir);

      expect(checks.every((check) => check.status === 'passed')).toBe(true);
    });
  });

  test('fails when the runtime artifact is missing', async () => {
    await withTempDir(async (tempDir) => {
      writeExtractedPackage(tempDir, false);

      const checks = await validateExtractedPackage(tempDir);

      expect(checks.find((check) => check.id === 'runtime:default-bin-contract')?.status).toBe(
        'failed'
      );
      expect(checks.find((check) => check.id === 'runtime:ts-entry-absent')?.status).toBe('passed');
    });
  });

  test('fails when public corpus capability coverage is missing', async () => {
    await withTempDir(async (tempDir) => {
      writeExtractedPackage(tempDir);
      writeJson(path.join(tempDir, 'corpus', 'public-url-corpus.json'), {
        cases: [
          {
            id: 'public-smoke',
            url: 'https://example.com/public-smoke.pdf',
            sha256: 'a'.repeat(64),
            source_label: 'public smoke fixture',
            source_homepage: 'https://example.com/',
            source_rights: 'test fixture',
            source_retrieved_at: '2026-06-21',
            capability_tags: ['selectable_text'],
          },
        ],
      });

      const checks = await validateExtractedPackage(tempDir);

      const publicCorpusCheck = checks.find(
        (check) => check.id === 'corpus:public-url-manifest-shape'
      );
      expect(publicCorpusCheck?.status).toBe('failed');
      expect(publicCorpusCheck?.evidence?.missing_required_capability_tags).toContain(
        'legacy_scan'
      );
    });
  });

  test('fails when public provider capability coverage is missing', async () => {
    await withTempDir(async (tempDir) => {
      writeExtractedPackage(tempDir);
      writeJson(path.join(tempDir, 'corpus', 'public-provider-accuracy.json'), {
        cases: [
          {
            id: 'provider-smoke',
            url: 'https://example.com/provider-smoke.pdf',
            sha256: 'b'.repeat(64),
            source_label: 'provider smoke fixture',
            source_homepage: 'https://example.com/',
            source_rights: 'test fixture',
            source_retrieved_at: '2026-06-21',
            capability_tags: ['visual_text'],
            regions: [
              {
                id: 'provider-region',
                page: 1,
                bounding_box: { left: 0, bottom: 0, right: 100, top: 100 },
                capability_tags: ['full_page_crop'],
                expected: { contains_text: ['provider'] },
              },
            ],
          },
        ],
      });

      const checks = await validateExtractedPackage(tempDir);

      const publicProviderCheck = checks.find(
        (check) => check.id === 'corpus:public-provider-manifest-shape'
      );
      expect(publicProviderCheck?.status).toBe('failed');
      expect(publicProviderCheck?.evidence?.missing_required_capability_tags).toContain(
        'scanned_page_triage'
      );
    });
  });

  test('fails when public corpus expected assertions are missing', async () => {
    await withTempDir(async (tempDir) => {
      writeExtractedPackage(tempDir);
      writeJson(path.join(tempDir, 'corpus', 'public-url-corpus.json'), {
        cases: [
          {
            id: 'public-smoke',
            url: 'https://example.com/public-smoke.pdf',
            sha256: 'a'.repeat(64),
            source_label: 'public smoke fixture',
            source_homepage: 'https://example.com/',
            source_rights: 'test fixture',
            source_retrieved_at: '2026-06-21',
            capability_tags: [
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
            ],
            expected: {
              contains_text: [],
            },
            read_pdf_options: {
              include_document_map: false,
              include_text_layer: false,
            },
          },
        ],
      });

      const checks = await validateExtractedPackage(tempDir);

      const publicCorpusCheck = checks.find(
        (check) => check.id === 'corpus:public-url-manifest-shape'
      );
      expect(publicCorpusCheck?.status).toBe('failed');
      expect(publicCorpusCheck?.evidence?.cases_with_expected_text).toBe(0);
      expect(publicCorpusCheck?.evidence?.cases_with_document_map_option).toBe(0);
      expect(publicCorpusCheck?.evidence?.cases_with_text_layer_option).toBe(0);
    });
  });

  test('fails when public provider region evidence contracts are weak', async () => {
    await withTempDir(async (tempDir) => {
      writeExtractedPackage(tempDir);
      writeJson(path.join(tempDir, 'corpus', 'public-provider-accuracy.json'), {
        cases: [
          {
            id: 'provider-smoke',
            url: 'https://example.com/provider-smoke.pdf',
            sha256: 'b'.repeat(64),
            source_label: 'provider smoke fixture',
            source_homepage: 'https://example.com/',
            source_rights: 'test fixture',
            source_retrieved_at: '2026-06-21',
            capability_tags: [
              'accessibility_diagram',
              'image_plus_text',
              'legacy_scan',
              'visual_text',
            ],
            regions: [
              {
                id: 'provider-region',
                page: 1,
                bounding_box: { left: 0, bottom: 0, right: 0, top: 100 },
                capability_tags: [
                  'diagram_context',
                  'full_page_crop',
                  'layout_diagram',
                  'scanned_page_triage',
                ],
                expected: { contains_text: [] },
              },
            ],
          },
        ],
      });

      const checks = await validateExtractedPackage(tempDir);

      const publicProviderCheck = checks.find(
        (check) => check.id === 'corpus:public-provider-manifest-shape'
      );
      expect(publicProviderCheck?.status).toBe('failed');
      expect(publicProviderCheck?.evidence?.regions_with_valid_bounding_boxes).toBe(0);
      expect(publicProviderCheck?.evidence?.regions_with_expected_kind).toBe(0);
      expect(publicProviderCheck?.evidence?.regions_with_expected_text).toBe(0);
      expect(publicProviderCheck?.evidence?.regions_with_min_confidence).toBe(0);
    });
  });

  test('fails when public provider region confidence or page contracts are outside scoreable bounds', async () => {
    await withTempDir(async (tempDir) => {
      writeExtractedPackage(tempDir);
      writeJson(path.join(tempDir, 'corpus', 'public-provider-accuracy.json'), {
        cases: [
          {
            id: 'provider-smoke',
            url: 'https://example.com/provider-smoke.pdf',
            sha256: 'b'.repeat(64),
            source_label: 'provider smoke fixture',
            source_homepage: 'https://example.com/',
            source_rights: 'test fixture',
            source_retrieved_at: '2026-06-21',
            capability_tags: [
              'accessibility_diagram',
              'image_plus_text',
              'legacy_scan',
              'visual_text',
            ],
            regions: [
              {
                id: 'provider-region',
                page: 1.5,
                bounding_box: { left: 0, bottom: 0, right: 100, top: 100 },
                capability_tags: [
                  'diagram_context',
                  'full_page_crop',
                  'layout_diagram',
                  'scanned_page_triage',
                ],
                expected: { contains_text: ['provider'], min_confidence: 1.5 },
              },
            ],
          },
        ],
      });

      const checks = await validateExtractedPackage(tempDir);

      const publicProviderCheck = checks.find(
        (check) => check.id === 'corpus:public-provider-manifest-shape'
      );
      expect(publicProviderCheck?.status).toBe('failed');
      expect(publicProviderCheck?.evidence?.regions_with_valid_bounding_boxes).toBe(0);
      expect(publicProviderCheck?.evidence?.regions_with_expected_kind).toBe(0);
      expect(publicProviderCheck?.evidence?.regions_with_expected_text).toBe(1);
      expect(publicProviderCheck?.evidence?.regions_with_min_confidence).toBe(0);
    });
  });

  test('finds the tarball path from bun pack output or destination contents', async () => {
    await withTempDir(async (tempDir) => {
      const tarballPath = path.join(tempDir, 'package.tgz');
      fs.writeFileSync(tarballPath, 'tgz', 'utf8');

      await expect(findPackedTarballPath(`\n${tarballPath}\n`, tempDir)).resolves.toBe(tarballPath);
      await expect(findPackedTarballPath('', tempDir)).resolves.toBe(tarballPath);
    });
  });
});
