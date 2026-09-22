# API Reference

PDF Reader MCP exposes an MCP server contract. The package entrypoint starts the
server; it is not an importable TypeScript SDK. Agents and clients should call
the MCP tools below over stdio or the optional HTTP transport.

The V3 API is organized around one smart default path. Agents call `read_pdf`
first; when no explicit `include_*` options are supplied, it profiles the PDF,
chooses high-value extraction options, and returns the structured document result in
one response. `search_pdf` stays separate for cheap literal evidence retrieval,
and `pdf_evidence` consolidates focused inspect, render, crop, OCR, and visual
analysis operations behind one specialist tool.

## Transports

| Setting | Description | Default |
| --- | --- | --- |
| `MCP_TRANSPORT` | `stdio` or `http` | `stdio` |
| `MCP_HTTP_HOST` | HTTP bind host when `MCP_TRANSPORT=http`. Defaults to loopback; set explicitly (and set `MCP_API_KEY`) to expose on other interfaces. | `127.0.0.1` |
| `MCP_HTTP_PORT` | HTTP port when `MCP_TRANSPORT=http` | `8080` |
| `MCP_API_KEY` | When set, every `/mcp` request must send a matching `X-API-Key` header or it is rejected with `401`. The `/mcp/health` check stays open. Leave unset only for a loopback-bound, single-tenant server. | unset |
| `MCP_CORS_ORIGIN` | Optional explicit CORS origin | unset |

> **Security:** The HTTP transport exposes every PDF tool to whoever can reach
> the port. It binds to loopback (`127.0.0.1`) by default. Before binding any
> non-loopback host, set `MCP_API_KEY` so callers must authenticate with an
> `X-API-Key` header, and restrict filesystem reach with `--allow-dir` /
> `MCP_PDF_ALLOWED_DIRS`. The server warns at startup if it binds a non-loopback
> host without a key.

## Tools

| Tool | Purpose |
| --- | --- |
| `read_pdf` | Primary V3 entrypoint. With only `sources`, auto-inspect and read the PDF in one call; with explicit `include_*` options, run precise manual extraction. |
| `search_pdf` | Search selectable text and optional OCR text with snippets, page numbers, offsets, bounding-box provenance, and routing evidence. |
| `pdf_evidence` | Focused evidence operations: `inspect`, `render_page`, `extract_regions`, `ocr_pages`, and `analyze_regions`. |

## Source Object

The V3 tools accept `sources` arrays so callers can batch local paths and URLs
through one request when the operation supports it.

```json
{
  "path": "/absolute/path/to/file.pdf"
}
```

```json
{
  "url": "https://example.com/file.pdf"
}
```

Use exactly one of `path` or `url`. URL loading is guarded by the HTTP, host,
private-IP, and size policies documented in the guide.

## `read_pdf`

`read_pdf` is the primary structured document result entrypoint. With only `sources`,
it defaults to automatic routing. Add `auto: false` or any explicit
`include_*` option when the caller wants exact manual control.

| Option | Type | Default | Output |
| --- | --- | --- | --- |
| `auto` | boolean | true when no explicit `include_*` options are supplied | Inspect each source and choose high-value extraction options before reading. |
| `auto_detail` | `"fast" \| "balanced" \| "full"` | `"balanced"` | Automatic extraction depth. `fast` returns the core document twin route, `balanced` adds trust and accessibility evidence, and `full` adds fuller text, HTML, structure, and AST outputs. |
| `sample_pages` | number | `5` in auto mode | Maximum pages sampled during automatic inspection. |
| `pages` | number array or range string | all pages when full text is requested | Page selection. |
| `include_full_text` | boolean | `false` | Concatenated text. |
| `include_metadata` | boolean | `true` | PDF metadata. |
| `include_page_count` | boolean | `true` | Total page count. |
| `include_images` | boolean | `false` | Embedded image metadata and base64 payloads. |
| `include_tables` | boolean | `false` | Selectable-text and OCR-derived tables with rows, cells, geometry, confidence, provenance, quality signals, and continuation hints. |
| `include_elements` | boolean | `false` | Structured text, image, and table elements. |
| `include_markdown` | boolean | `false` | Markdown rendering. |
| `include_html` | boolean | `false` | HTML rendering. |
| `include_chunks` | boolean | `false` | Citation-ready chunks. |
| `include_text_layer` | boolean | `false` | Direction-aware run, line, word, and character evidence with metadata coverage counts. |
| `include_layout_diagnostics` | boolean | `false` | Reading-order and page-layout confidence. |
| `include_document_map` | boolean | `false` | Page, element, chunk, OCR, visual candidate, visual enrichment, safety, trust signal-index, accessibility issue-index, and routing map. |
| `include_document_ast` | boolean | `false` | Semantic AST for page, section, paragraph, list, caption, header, footer, table, image, chart, formula, and figure nodes, including numbered/appendix headings and above/below/side caption evidence links. |
| `include_safety_findings` | boolean | `false` | Prompt-injection, hidden or near-invisible text geometry, and visual-spoofing findings. |
| `include_trust_report` | boolean | `false` | Consolidated risk report with page-level signals, category counts, page-risk counts, routing guidance, and optional document-map trust signal routing. |
| `trust_report_redaction` | `"standard" \| "strict" \| "off"` | `"standard"` | Redaction policy for trust-report evidence snippets. `standard` redacts common secrets and personal identifiers, `strict` also redacts phone-like values and IPv4 addresses, and `off` preserves snippets while marking the policy explicitly. |
| `include_accessibility_report` | boolean | `false` | Tagged-PDF, image-alt, form, permission, tag-visible coverage, issue-summary, page-grade routing, and optional document-map issue-index signals. |
| `include_ocr_text_layer` | boolean | `false` | OCR page text and PDF-coordinate word boxes from a configured OCR provider. OCR word boxes can also feed table extraction when `include_tables` is enabled. |
| `include_visual_enrichments` | boolean | `false` | Bbox-grounded visual-region candidates plus provider-normalized table/image and caption-derived visual region evidence, including side-caption candidates, when a provider is configured. |

## Table Quality

When `include_tables` is enabled, each table may include `quality`:

| Field | Meaning |
| --- | --- |
| `completeness` | Combined non-empty-cell and row-alignment score. |
| `nonEmptyCellRatio` | Ratio of cells with text. |
| `cellBoundingBoxCoverage` | Ratio of cells with bounding boxes. |
| `inferredCellRatio` | Ratio of cells inferred by the table grid model. |
| `rowAlignment` | Alignment score against detected column boundaries. |
| `rowSpacingConsistency` | Consistency of row spacing. |
| `cellBoundingBoxCount` | Number of cells with bounding boxes. |
| `inferredCellCount` | Number of inferred cells. |
| `missingCellCount` | Number of empty cells. |
| `mergedCellCandidateCount` | Number of cells with inferred spans. |
| `signals` | Machine-readable quality signals such as `complete_grid`, `missing_cells`, `merged_cell_candidates`, `incomplete_cell_geometry`, `irregular_row_spacing`, `multi_page_continuation_candidate`, and `low_confidence`. |
| `warnings` | Human-readable routing guidance for weak table evidence. |

Tables also include `provenance.source`. `selectable_text` means the table came
from PDF text coordinates. `ocr_text_layer` means it came from OCR word boxes
linked through `ocr_source_render_evidence_id`. OCR-derived tables are merged by
bounding-box overlap, so duplicate OCR evidence is suppressed while distinct
scanned tables on a mixed page are retained.

Agents should use `incomplete_cell_geometry`, sparse-cell, merged-cell,
irregular-spacing, and low-confidence warnings as a cue to request
`pdf_evidence` operation `extract_regions`, `render_page`, or
`analyze_regions` before making cell-level claims.

## `pdf_evidence`

`pdf_evidence` is the single specialist evidence tool in V3. It exists for
focused follow-up work after `read_pdf` or `search_pdf` exposes the page,
region, OCR, or provider evidence an agent needs.

| Option | Type | Used by |
| --- | --- | --- |
| `operation` | `"inspect" \| "render_page" \| "extract_regions" \| "ocr_pages" \| "analyze_regions"` | Required. |
| `sources` | array | Required. Each source uses `path` or `url`; `pages` is accepted for page-scoped operations and `regions` is required for region operations. |
| `sample_pages` | number | `inspect` |
| `include_metadata` | boolean | `inspect` |
| `scale` | number | `render_page`, `extract_regions`, `ocr_pages`, `analyze_regions` |
| `max_pages` | number | `render_page`, `ocr_pages` |
| `max_regions` | number | `extract_regions`, `analyze_regions` |
| `max_pixels_per_page` | number | image-producing operations |
| `include_image` | boolean | `render_page`, `extract_regions` |
| `timeout_ms` | number | `ocr_pages`, `analyze_regions` |
| `max_output_chars` | number | `ocr_pages`, `analyze_regions` |
| `languages` | string array | `ocr_pages`, `analyze_regions` |

Inspect:

```json
{
  "operation": "inspect",
  "sources": [{ "path": "/absolute/path/to/file.pdf" }],
  "sample_pages": 5,
  "include_metadata": true
}
```

Render pages:

```json
{
  "operation": "render_page",
  "sources": [{ "path": "/absolute/path/to/file.pdf", "pages": "1-2" }],
  "scale": 2,
  "max_pages": 2
}
```

Crop or analyze regions:

```json
{
  "operation": "extract_regions",
  "sources": [{
    "path": "/absolute/path/to/file.pdf",
    "regions": [{
      "id": "table-1",
      "page": 1,
      "bounding_box": { "left": 72, "bottom": 420, "right": 540, "top": 620 },
      "padding": 8
    }]
  }],
  "scale": 2,
  "max_regions": 20
}
```

Use `operation: "analyze_regions"` with the same `regions` shape when a
configured visual provider should normalize table, formula, chart, figure, or
image-description evidence. Use `operation: "ocr_pages"` with page-scoped
sources when a workflow needs standalone OCR output.

## Provider Adapters

The server does not bundle OCR, formula, chart, or vision models. It provides
stable local adapters so deployments can choose their own engines.

When `include_visual_enrichments` is enabled without a configured visual
provider, `read_pdf` still returns `visual_enrichment_candidates`. These records
contain stable region IDs, PDF-coordinate boxes, target types, caption evidence,
and routing signals for follow-up `pdf_evidence` `extract_regions` or
`analyze_regions` operations.

| Capability | Configuration |
| --- | --- |
| OCR command provider | `MCP_PDF_OCR_COMMAND`, `MCP_PDF_OCR_ARGS_JSON`, `MCP_PDF_OCR_TIMEOUT_MS`, `MCP_PDF_OCR_MAX_OUTPUT_CHARS` |
| OCR preset | `MCP_PDF_OCR_PRESET=tesseract` or `tesseract-tsv` |
| Visual-region command provider | `MCP_PDF_REGION_ANALYSIS_COMMAND`, `MCP_PDF_REGION_ANALYSIS_ARGS_JSON`, `MCP_PDF_REGION_ANALYSIS_TIMEOUT_MS`, `MCP_PDF_REGION_ANALYSIS_MAX_OUTPUT_CHARS` |
| Visual-region HTTP provider | `MCP_PDF_REGION_ANALYSIS_HTTP_URL`, optional `MCP_PDF_REGION_ANALYSIS_HTTP_HEADERS_JSON` |
| Visual-region Ollama preset | `MCP_PDF_REGION_ANALYSIS_PRESET=ollama`, `MCP_PDF_REGION_ANALYSIS_OLLAMA_MODEL`, optional `MCP_PDF_REGION_ANALYSIS_OLLAMA_URL` |
| Visual-region OpenAI-compatible preset | `MCP_PDF_REGION_ANALYSIS_PRESET=openai-compatible`, `MCP_PDF_REGION_ANALYSIS_OPENAI_MODEL`, `MCP_PDF_REGION_ANALYSIS_OPENAI_URL`, optional `MCP_PDF_REGION_ANALYSIS_OPENAI_API_KEY` |
| Visual-region LM Studio preset | `MCP_PDF_REGION_ANALYSIS_PRESET=lmstudio`, `MCP_PDF_REGION_ANALYSIS_LMSTUDIO_MODEL`, optional `MCP_PDF_REGION_ANALYSIS_LMSTUDIO_URL` |
| Visual-region llama.cpp preset | `MCP_PDF_REGION_ANALYSIS_PRESET=llamacpp`, `MCP_PDF_REGION_ANALYSIS_LLAMACPP_MODEL`, optional `MCP_PDF_REGION_ANALYSIS_LLAMACPP_URL` |

Provider responses are normalized into the same evidence model used by
`read_pdf`, `pdf_evidence` operation `analyze_regions`, and the benchmark
harness.

## Quality Gates

Use these commands before publishing:

```bash
bun run check
bun run typecheck
bun run test:cov
bun run build
bun run package:smoke
bun run docs:build
bun run benchmark:quality
bun run benchmark:providers
MCP_PDF_BENCHMARK_OUTPUT_DIR=./benchmark-artifacts bun run benchmark:all
MCP_PDF_BENCHMARK_OUTPUT_DIR=./benchmark-artifacts bun run benchmark:release-gate
bun run release:preflight
```

Set `MCP_PDF_BENCHMARK_OUTPUT_DIR` to persist benchmark JSON artifacts for
release review:

```bash
MCP_PDF_BENCHMARK_OUTPUT_DIR=./benchmark-artifacts bun run benchmark:release-artifacts
```

`benchmark:release-gate` reads those artifacts and fails until deterministic
final-bar coverage, mandatory corpus archetype evidence, deterministic
provider-manifest crop evidence, deterministic provider-manifest scoring
evidence, and installed-provider final-bar evidence are complete.
`package:smoke` packs the package locally and verifies that the tarball contains
the executable `dist/index.js` runtime artifact with matching `bin` and
`exports` metadata. It also verifies that public evidence manifests keep source
metadata, SHA256 values, capability tags, expected corpus assertions, required
read options, and provider-region bbox/normalized-confidence/text contracts in
the packed package. Public provider manifests must also retain expected visual
kind coverage for chart, diagram, figure, formula, image, and table regions.
`release:preflight` runs the full publish gate and requires strict installed
provider certification before publishing can proceed.

The repository release workflow installs Tesseract for the `tesseract-tsv` OCR
profile and configures `scripts/reference-region-analysis-provider.mjs` for the
visual `visual-full-fidelity` fixture profile before running strict benchmark
artifacts and the release gate. The reference visual provider is a deterministic
contract-certification adapter, not a bundled general-purpose vision model.
The CI workflow runs the same strict benchmark artifact and release-gate path as
a non-publishing evidence job, so pull requests can fail before release if OCR,
visual-provider certification, provider-manifest crop substrate evidence, or
provider-manifest scoring evidence regresses.

`benchmark:providers` reports skipped providers when local engines are not
installed. Configure OCR or visual-region adapters to certify installed-provider
capabilities. Its JSON report also emits
per-provider `quality` metrics, `final_bar_provider_evidence_summary`, and
`final_bar_provider_evidence`, mapping provider certification profiles to the
SOTA final-bar capabilities that require installed-provider evidence.

`benchmark:corpus` emits `pdf_corpus_benchmark.json`, covering the checked-in
sample PDF plus runtime-generated reading-order, scanned-OCR routing, and
OCR-derived table archetypes. Each case carries `capability_tags`, and the
artifact emits `capability_summary` so reviewers can inspect evidence by
document-intelligence area. The release gate requires all corpus archetypes,
checked-in and runtime-generated fixture diversity, case-level capability tags,
required capability-summary coverage, and passing per-case assertions. It can
also include operator-supplied real PDFs through
`--corpus-manifest` or `MCP_PDF_CORPUS_MANIFEST`; those external cases are
reported in the same artifact but are not required by the deterministic CI
release gate. Manifest cases may reference local `path` files or public `url`
files. URL cases require `sha256`, use a content-addressed cache, and download
only when `--allow-corpus-downloads` or `MCP_PDF_CORPUS_ALLOW_DOWNLOADS=true`
is set. Private, loopback, and link-local URL hosts are blocked by default and
only allowed with the existing `--allow-private-ips` /
`MCP_PDF_ALLOW_PRIVATE_IPS=true` development override. The published package
includes `corpus/public-url-corpus.json`, an opt-in manifest of official and
publicly available PDFs with source metadata, pinned SHA256 values, and
required capability tags. The package smoke gate verifies that the published
tarball keeps the public corpus and provider capability coverage manifests, and
that public corpus cases retain expected text/page/text-volume assertions plus
document-map and text-layer read options. The checked-in public corpus includes
official form/guidance/technical-report evidence plus public statistical-report
and research-paper evidence for chart, formula, and table-heavy documents.

`benchmark:provider-manifest` emits `pdf_provider_manifest_benchmark.json`
when `--provider-manifest` or `MCP_PDF_PROVIDER_MANIFEST` points at a provider
accuracy manifest. It crops the declared regions through the same renderer and
provider-normalization path as `pdf_evidence` operation `analyze_regions`, then
scores kind, text,
confidence, structured table/formula/chart evidence, and crop provenance
against the manifest expectations. Case and region `capability_tags` flow into
the artifact-level `capability_summary`, so release reviewers can see which
public visual capabilities the manifest exercises instead of reading only a
single aggregate score. URL cases require `sha256`, use the same
content-addressed cache, and download only when
`--allow-provider-manifest-downloads` or
`MCP_PDF_PROVIDER_MANIFEST_ALLOW_DOWNLOADS=true` is set. The published package
includes `corpus/public-provider-accuracy.json`, an opt-in manifest of public
PDF crop regions with source metadata, pinned SHA256 values, and required
case/region capability tags. Package smoke also requires every published
provider region to keep positive-area bounding boxes, expected text terms, and
normalized minimum confidence thresholds so the manifest remains scoreable
after publish. The same gate requires the published provider manifest to keep
expected-kind coverage for chart, diagram, figure, formula, image, and table
regions.
`benchmark:release-artifacts` also runs provider-manifest scoring against a
deterministic local fixture manifest with the configured reference provider, so
`benchmark:release-gate` requires table, formula, chart, figure, image,
confidence, text, crop-provenance, and capability-summary assertions without
network downloads.

`benchmark:provider-manifest-crops` emits
`pdf_provider_manifest_crop_benchmark.json` for the same manifest shape without
requiring a visual-region provider. It downloads and checksum-validates opt-in
URL cases when enabled, renders the declared pages, verifies each region crop,
and records crop byte length, pixel bounds, render provenance, source metadata,
and capability summaries. Use it when release evidence needs to prove the
public PDF crop substrate before provider/model accuracy is evaluated.
`benchmark:release-artifacts` also runs this benchmark against a deterministic
local fixture manifest, so `benchmark:release-gate` requires crop-substrate
evidence even when CI does not perform network downloads or use a local model.

`benchmark:quality` also emits `final_bar_coverage_summary` and
`final_bar_coverage` so release reviewers can see which SOTA final-bar
capabilities are covered by deterministic fixtures and which still require
installed-provider benchmark evidence.
