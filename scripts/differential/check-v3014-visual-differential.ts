#!/usr/bin/env bun

import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PNG } from 'pngjs';
import { resolveCargoReleaseDir } from '../native/cargo-target-dir.ts';

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
type Content = { type: string; text?: string; data?: string; mimeType?: string };
type ToolResult = { content: Content[]; isError?: boolean };

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, '../..');
const fixtureDir = join(repoRoot, 'test/fixtures/differential');
const corpusPath = join(scriptDir, 'fixtures/v3014-visual-corpus.json');
const oraclePath = join(scriptDir, 'fixtures/v3014-visual-oracle.json');
const fixtureManifestPath = join(scriptDir, 'fixtures/v3014-visual-fixtures.json');
const providerPath = join(scriptDir, 'reference-ocr-provider.ts');
const regionProviderPath = join(scriptDir, 'reference-region-analysis-provider.ts');
const serverPath = join(resolveCargoReleaseDir(repoRoot), 'anymd');
const outputFlag = process.argv.indexOf('--output');
const outputPath = outputFlag >= 0 ? process.argv[outputFlag + 1] : undefined;

const corpus = JSON.parse(readFileSync(corpusPath, 'utf8')) as {
  cases: Array<{ id: string; tool?: 'read_pdf'; input: Record<string, unknown> }>;
};
const oracle = JSON.parse(readFileSync(oraclePath, 'utf8')) as {
  baseline: {
    tag: string;
    commit: string;
    tree: string;
    bunLockSha256: string;
    entrypointSha256: Record<string, string>;
  };
  expectations: Record<string, Json>;
  providerSha256: string;
  regionProviderSha256: string;
};
const fixtureManifest = JSON.parse(readFileSync(fixtureManifestPath, 'utf8')) as {
  fixtures: Array<{ path: string; bytes: number; sha256: string }>;
};
const sha256 = (bytes: Uint8Array | string): string =>
  createHash('sha256').update(bytes).digest('hex');
const git = (...args: string[]): Buffer => {
  const result = spawnSync('git', args, { cwd: repoRoot });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};

function verifyAuthority(): Record<string, string> {
  const commit = git('rev-list', '-n', '1', oracle.baseline.tag).toString().trim();
  if (commit !== oracle.baseline.commit) throw new Error('v3.0.14 visual baseline tag moved');
  const tree = git('rev-parse', `${commit}^{tree}`).toString().trim();
  if (tree !== oracle.baseline.tree) throw new Error('v3.0.14 visual baseline tree mismatch');
  if (sha256(git('show', `${commit}:bun.lock`)) !== oracle.baseline.bunLockSha256) {
    throw new Error('v3.0.14 visual baseline lock mismatch');
  }
  for (const [path, expected] of Object.entries(oracle.baseline.entrypointSha256)) {
    if (sha256(git('show', `${commit}:${path}`)) !== expected) {
      throw new Error(`v3.0.14 visual baseline entrypoint mismatch: ${path}`);
    }
  }
  for (const fixture of fixtureManifest.fixtures) {
    const path = join(repoRoot, fixture.path);
    const bytes = readFileSync(path);
    if (bytes.length !== fixture.bytes || sha256(bytes) !== fixture.sha256) {
      throw new Error(`visual fixture mismatch: ${fixture.path}`);
    }
  }
  const ids = corpus.cases.map((entry) => entry.id);
  if (ids.length !== 16 || new Set(ids).size !== ids.length) {
    throw new Error(`visual/OCR/analysis corpus must contain 16 unique cases (got ${ids.length})`);
  }
  if (sha256(readFileSync(providerPath)) !== oracle.providerSha256) {
    throw new Error('reference OCR provider digest mismatch');
  }
  if (sha256(readFileSync(regionProviderPath)) !== oracle.regionProviderSha256) {
    throw new Error('reference region analysis provider digest mismatch');
  }
  if (JSON.stringify(ids.sort()) !== JSON.stringify(Object.keys(oracle.expectations).sort())) {
    throw new Error('visual corpus and oracle IDs differ');
  }
  return {
    baselineCommit: commit,
    baselineTree: tree,
    corpusSha256: sha256(readFileSync(corpusPath)),
    oracleSha256: sha256(readFileSync(oraclePath)),
    fixtureManifestSha256: sha256(readFileSync(fixtureManifestPath)),
  };
}

function materialize(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(materialize);
  if (value && typeof value === 'object') {
    const output: Record<string, unknown> = {};
    for (const [key, entry] of Object.entries(value as Record<string, unknown>)) {
      output[key === 'fixture' ? 'path' : key] =
        key === 'fixture' && typeof entry === 'string' ? join(fixtureDir, entry) : materialize(entry);
    }
    return output;
  }
  return value;
}

function stable(value: Json): Json {
  if (Array.isArray(value)) return value.map(stable);
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([key, entry]) => [key, stable(entry)])
    );
  }
  return value;
}

function errorCategory(message: string): string {
  const lower = message.toLowerCase();
  if (lower.includes('right > left') || lower.includes('top > bottom')) {
    return 'invalid_bounding_box';
  }
  if (
    lower.includes('not found') ||
    lower.includes('no such file') ||
    lower.includes('unable to access')
  ) {
    return 'file_not_found';
  }
  return 'unknown_error';
}

function imageFacts(content: Content): Record<string, Json> {
  const bytes = Buffer.from(content.data ?? '', 'base64');
  const png = PNG.sync.read(bytes);
  const sample = (x: number, y: number): number[] => {
    const offset = (y * png.width + x) * 4;
    return Array.from(png.data.subarray(offset, offset + 4));
  };
  return {
    mime_type: content.mimeType ?? null,
    raw_base64: !(content.data ?? '').startsWith('data:'),
    png_signature: bytes.subarray(0, 8).equals(Buffer.from('\x89PNG\r\n\x1a\n', 'binary')),
    width: png.width,
    height: png.height,
    byte_length: bytes.length,
    samples: [
      sample(Math.floor(png.width / 4), Math.floor(png.height / 4)),
      sample(Math.floor((png.width * 3) / 4), Math.floor(png.height / 4)),
      sample(Math.floor(png.width / 4), Math.floor((png.height * 3) / 4)),
      sample(Math.floor((png.width * 3) / 4), Math.floor((png.height * 3) / 4)),
    ],
  } as Record<string, Json>;
}

function tableProjectionFacts(
  data: Record<string, unknown>,
  map: Record<string, unknown> | undefined
): Json {
  const tables = (data.table_info ?? data.tables) as Array<Record<string, unknown>> | undefined;
  const elements = (data.elements ?? []) as Array<Record<string, unknown>>;
  const chunks = (data.chunks ?? []) as Array<Record<string, unknown>>;
  const ast = data.document_ast as Record<string, unknown> | undefined;
  if (!tables && !elements.some((element) => element.type === 'table') && !ast) return null;
  return {
    tables: (tables ?? []).map((table) => ({
      page: table.page ?? null,
      tableIndex: table.tableIndex ?? null,
      rowCount: table.rowCount ?? null,
      colCount: table.colCount ?? null,
      cellCount:
        table.cellCount ?? (Array.isArray(table.cells) ? table.cells.length : null),
      provenance: (table.provenance ?? null) as Json,
      quality: (table.quality ?? null) as Json,
    })) as Json,
    elements: elements
      .filter((element) => element.type === 'table')
      .map((element) => ({
        id: element.id ?? null,
        page: element.page ?? null,
        provenance: (element.provenance ?? null) as Json,
        rows: ((element.table as Record<string, unknown> | undefined)?.rows ?? null) as Json,
      })) as Json,
    chunks: chunks
      .filter((chunk) => chunk.strategy === 'table')
      .map((chunk) => ({
        text: chunk.text ?? null,
        element_ids: (chunk.element_ids ?? null) as Json,
        strategy: chunk.strategy ?? null,
      })) as Json,
    markdown_has_table:
      typeof data.markdown === 'string' && data.markdown.includes('| Metric | Value |'),
    html_has_table: typeof data.html === 'string' && data.html.includes('<table'),
    ast_table_count:
      ((ast?.summary as Record<string, unknown> | undefined)?.table_count as Json) ?? null,
    ast_has_ocr_table_provenance: JSON.stringify(ast ?? {}).includes(
      '"source":"ocr_text_layer"'
    ),
    map: map
      ? ({
          has_table_structure:
            Array.isArray(map.layers) && map.layers.includes('table_structure'),
          has_citation_chunks:
            Array.isArray(map.layers) && map.layers.includes('citation_chunks'),
          page_table_count:
            ((((map.pages as Array<Record<string, unknown>> | undefined) ?? [])[0]
              ?.table_count as Json) ?? null),
          table_element_sources: ((
            (map.elements as Array<Record<string, unknown>> | undefined) ?? []
          )
            .filter((element) => element.type === 'table')
            .map(
              (element) =>
                (element.provenance as Record<string, unknown> | undefined)?.source ?? null
            ) as Json),
        } as Json)
      : null,
  };
}

function canonicalize(result: ToolResult, tool?: 'read_pdf'): Json {
  if (result.isError) {
    return {
      outcome: 'error',
      category: errorCategory(result.content[0]?.text ?? ''),
    };
  }
  const payload = JSON.parse(result.content[0]?.text ?? '{}') as Record<string, unknown>;
  if (tool === 'read_pdf') {
    const readResults = ((payload.results ?? []) as Array<Record<string, unknown>>).map((source) => {
      if (!source.success) {
        return {
          source: basename(String(source.source ?? '')),
          success: false,
          category: errorCategory(String(source.error ?? '')),
        };
      }
      const data = (source.data ?? {}) as Record<string, unknown>;
      const layer = data.ocr_text_layer as Record<string, unknown> | undefined;
      const map = data.document_map as Record<string, unknown> | undefined;
      return {
        source: basename(String(source.source ?? '')),
        success: true,
        ocr_text_layer: layer
          ? {
              profile: layer.profile,
              pages: layer.pages,
              summary: layer.summary,
              warnings: layer.warnings ?? [],
            }
          : null,
        document_map: map
          ? {
              has_ocr_layer:
                Array.isArray(map.layers) && map.layers.includes('ocr_text_layer'),
              needs_ocr_pages: (map.routing as Record<string, unknown>)?.needs_ocr_pages,
              ocr_applied_pages: (map.routing as Record<string, unknown>)?.ocr_applied_pages,
              ocr_page_count: (map.summary as Record<string, unknown>)?.ocr_page_count,
              ocr_text_chars: (map.summary as Record<string, unknown>)?.ocr_text_chars,
            }
          : null,
        table_projection: tableProjectionFacts(data, map),
      };
    });
    return {
      outcome: 'success',
      content_ocr: result.content
        .filter((content) => content.type === 'text' && content.text?.startsWith('[Page '))
        .map((content) => content.text ?? null) as Json,
      results: readResults as Json,
    };
  }
  const images = result.content.filter((content) => content.type === 'image').map(imageFacts);
  const results = ((payload.results ?? []) as Array<Record<string, unknown>>).map((source) => {
    if (!source.success) {
      return {
        source: basename(String(source.source ?? '')),
        success: false,
        category: errorCategory(String(source.error ?? '')),
      };
    }
    const output: Record<string, unknown> = {
      source: basename(String(source.source ?? '')),
      success: true,
      num_pages: source.num_pages,
      warnings: source.warnings ?? [],
    };
    if (Array.isArray(source.rendered_pages)) {
      output.rendered_pages = source.rendered_pages.map((page: Record<string, unknown>) => {
        const provenance = page.provenance as Record<string, unknown>;
        if (provenance?.engine !== 'hayro' || provenance?.renderer !== 'hayro/vello_cpu') {
          throw new Error('Rust render provenance is not truthful');
        }
        const index = page.image_content_index as number | undefined;
        const image = index === undefined ? undefined : images[index - 1];
        const facts = image
          ? {
              ...image,
              byte_length_consistent: image.byte_length === page.byte_length,
              byte_length: undefined,
            }
          : null;
        return {
          page: page.page,
          evidence_id: page.evidence_id,
          width: page.width,
          height: page.height,
          scale: page.scale,
          pixel_count: page.pixel_count,
          format: page.format,
          mime_type: page.mime_type,
          rotation: page.rotation,
          provenance_source: provenance.source,
          image_content_index: index ?? null,
          inline_data_absent: !('data' in page),
          image: facts,
        };
      });
    }
    if (Array.isArray(source.regions)) {
      output.regions = source.regions.map((region: Record<string, unknown>) => {
        const provenance = region.provenance as Record<string, unknown>;
        if (provenance?.engine !== 'hayro' || provenance?.renderer !== 'hayro/vello_cpu') {
          throw new Error('Rust crop provenance is not truthful');
        }
        const index = region.image_content_index as number | undefined;
        const image = index === undefined ? undefined : images[index - 1];
        const facts = image
          ? {
              ...image,
              byte_length_consistent: image.byte_length === region.byte_length,
              byte_length: undefined,
            }
          : null;
        return {
          region_id: region.region_id,
          page: region.page,
          evidence_id: region.evidence_id,
          source_bounding_box: region.source_bounding_box,
          crop_pixels: region.crop_pixels,
          scale: region.scale,
          format: region.format,
          mime_type: region.mime_type,
          provenance_source: provenance.source,
          page_render_evidence_id: provenance.page_render_evidence_id,
          image_content_index: index ?? null,
          inline_data_absent: !('data' in region),
          image: facts,
        };
      });
    }
    if (Array.isArray(source.ocr_pages)) {
      output.ocr_pages = source.ocr_pages.map((page: Record<string, unknown>) => {
        const provenance = page.provenance as Record<string, unknown>;
        if (provenance?.engine !== 'external-command' || provenance?.source !== 'ocr-provider') {
          throw new Error('Rust OCR provider provenance is not truthful');
        }
        return {
          page: page.page,
          text: page.text,
          confidence: page.confidence ?? null,
          language: page.language ?? null,
          words: page.words ?? [],
          provider: page.provider,
          source_render_evidence_id: page.source_render_evidence_id,
          source_render_scale: page.source_render_scale,
          source_render_width: page.source_render_width,
          source_render_height: page.source_render_height,
          warnings: page.warnings ?? [],
          provenance: page.provenance,
        };
      });
    }
    if (Array.isArray(source.region_analyses)) {
      output.region_analyses = source.region_analyses.map((region: Record<string, unknown>) => {
        const provenance = region.provenance as Record<string, unknown>;
        if (
          provenance?.engine !== 'external-command' ||
          provenance?.source !== 'region-analysis-provider'
        ) {
          throw new Error('Rust region analysis provider provenance is not truthful');
        }
        return {
          region_id: region.region_id,
          page: region.page,
          kind: region.kind,
          description: region.description ?? null,
          text: region.text ?? null,
          markdown: region.markdown ?? null,
          confidence: region.confidence ?? null,
          table: region.table ?? null,
          formula: region.formula ?? null,
          chart: region.chart ?? null,
          warnings: region.warnings ?? [],
          provider: region.provider,
          source_crop_evidence_id: region.source_crop_evidence_id,
          source_bounding_box: region.source_bounding_box,
          crop_pixels: region.crop_pixels,
          scale: region.scale,
          provenance: region.provenance,
        };
      });
    }
    return output;
  });
  return {
    outcome: 'success',
    profile: (payload.profile ?? null) as Json,
    options: (
      payload.render_options ??
      payload.crop_options ??
      payload.ocr_options ??
      payload.analysis_options ??
      null
    ) as Json,
    content_count: result.content.length,
    results: results as Json,
  };
}

async function main() {
  if (!existsSync(serverPath)) throw new Error(`missing Rust server: ${serverPath}`);
  const authority = verifyAuthority();
  const child = spawn(serverPath, [], {
    cwd: repoRoot,
    stdio: ['pipe', 'pipe', 'inherit'],
    env: {
      ...process.env,
      PDF_READER_MCP_TRANSPORT: '',
      MCP_TRANSPORT: '',
      MCP_PDF_OCR_COMMAND: process.execPath,
      MCP_PDF_OCR_ARGS_JSON: JSON.stringify([
        providerPath,
        '{input}',
        '{page}',
        '{languages}',
      ]),
      MCP_PDF_REGION_ANALYSIS_COMMAND: process.execPath,
      MCP_PDF_REGION_ANALYSIS_ARGS_JSON: JSON.stringify([
        regionProviderPath,
        '{input}',
        '{page}',
        '{region_id}',
        '{evidence_id}',
        '{languages}',
      ]),
    },
  });
  let buffer = '';
  const pending = new Map<number, (value: Record<string, unknown>) => void>();
  child.stdout.on('data', (chunk: Buffer) => {
    buffer += chunk.toString();
    while (buffer.includes('\n')) {
      const index = buffer.indexOf('\n');
      const line = buffer.slice(0, index).trim();
      buffer = buffer.slice(index + 1);
      if (!line) continue;
      const response = JSON.parse(line) as Record<string, unknown>;
      const id = Number(response.id);
      pending.get(id)?.(response);
      pending.delete(id);
    }
  });
  const request = (id: number, method: string, params: unknown) =>
    new Promise<Record<string, unknown>>((resolve, reject) => {
      const timeout = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`Rust MCP request ${id} timed out`));
      }, 30_000);
      pending.set(id, (value) => {
        clearTimeout(timeout);
        resolve(value);
      });
      child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
    });

  try {
    await request(1, 'initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'visual-differential', version: '1' },
    });
    child.stdin.write(
      `${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`
    );
    const observations: Record<string, Json> = {};
    const failures: Array<{ id: string; expected: Json; actual: Json }> = [];
    for (const [index, entry] of corpus.cases.entries()) {
      const response = await request(index + 10, 'tools/call', {
        name: entry.tool ?? 'pdf_evidence',
        arguments: materialize(entry.input),
      });
      if (response.error) throw new Error(`Rust MCP error for ${entry.id}: ${JSON.stringify(response.error)}`);
      const actual = stable(canonicalize(response.result as ToolResult, entry.tool));
      const expected = stable(oracle.expectations[entry.id]!);
      observations[entry.id] = actual;
      if (JSON.stringify(actual) !== JSON.stringify(expected)) {
        failures.push({ id: entry.id, expected, actual });
      }
    }
    const candidateSha = git('rev-parse', 'HEAD').toString().trim();
    if (process.env.CANDIDATE_SHA && process.env.CANDIDATE_SHA !== candidateSha) {
      throw new Error(
        `candidate SHA assertion mismatch: expected ${process.env.CANDIDATE_SHA}, executed ${candidateSha}`
      );
    }
    const report = {
      schemaVersion: 1,
      profile: 'pdf_reader_v3014_visual_result',
      candidateSha,
      ...authority,
      caseCount: corpus.cases.length,
      passed: corpus.cases.length - failures.length,
      skipped: 0,
      pass: failures.length === 0,
      observations,
      failures,
    };
    const serialized = `${JSON.stringify(report, null, 2)}\n`;
    if (outputPath) await Bun.write(outputPath, serialized);
    console.log(serialized.trimEnd());
    if (failures.length > 0) process.exitCode = 1;
  } finally {
    child.kill('SIGTERM');
  }
}

await main();
