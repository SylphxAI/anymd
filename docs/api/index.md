# API Reference

PDF Reader MCP exposes an MCP server contract. The package entrypoint starts the
server; it is not an importable TypeScript SDK. Agents and clients should call
the MCP tools below over stdio or the optional HTTP transport.

Agents call `read_pdf` first. With only `sources`, the **fast** preset returns
markdown, tables, chunks, a document map, page geometry, layout, and semantic
hints. It does not profile the file, sample pages, or run OCR. `search_pdf` is
the cheap literal lookup. `pdf_evidence` is the specialist for inspect, render,
crop, OCR, and visual analysis.

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
| `read_pdf` | Primary read. Sources only uses fast. `profile` selects quality or research. Explicit `include_*` options, or `auto: false`, stay manual. |
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

`read_pdf` reads every page you asked for. Sources only, including a source
materialized as `"pages": null`, uses **fast**. A real page list is a filter,
not a mode switch.

| Profile | When | Returns | Does not return |
| --- | --- | --- | --- |
| `fast` | sources only, or `"profile": "fast"` | markdown, tables, chunks, document map, page geometry, layout, semantic hints (on elements), metadata, page count | trust, safety, accessibility, text layer, HTML, document AST, OCR |
| `quality` | `"profile": "quality"` | fast, plus text layer, elements, HTML, document AST, outline, annotations, forms, attachments, structure tree, permissions, full text, page labels | audits, OCR, rendering |
| `research` | `"profile": "research"` | quality, plus safety findings, trust report, accessibility report | OCR, rendering |
| `balanced` | `"auto": true` and no `profile` / `auto_detail` | fast, plus safety, trust, and accessibility | the quality structure extras, OCR |
| `full` | `"auto_detail": "full"` | structure and audits together | OCR, rendering |

`auto_detail` wins over `profile`. `auto: false` or any `include_*` flag does
not fill a preset. Metadata and page count stay on unless the caller sets them
to false. OCR is `include_ocr_text_layer: true` or `pdf_evidence`.

| Option | Type | Default | Output |
| --- | --- | --- | --- |
| `profile` | `"fast" \| "quality" \| "research"` | `fast` when no `include_*` and `auto` is omitted | Named preset. Ignored when `auto` is false or any `include_*` is set. |
| `auto` | boolean | omitted (fast) | `true` selects balanced when `profile` and `auto_detail` are omitted. `false` is manual. |
| `auto_detail` | `"fast" \| "balanced" \| "full"` | unset | Wins over `profile`. `full` adds structure and audits. Never enables OCR. |
| `sample_pages` | number | unused | Accepted for compatibility. `read_pdf` presets do not sample. Use `sources[].pages`, or `pdf_evidence` inspect. |
| `pages` | number array or range string, on each source | every page | Page filter. Does not disable the preset. |
| `include_full_text` | boolean | on in quality | Concatenated text. |
| `include_metadata` | boolean | `true` | PDF metadata. |
| `include_page_count` | boolean | `true` | Total page count. |
| `include_images` | boolean | `false` | Embedded image metadata and base64 payloads. |
| `include_tables` | boolean | on in fast | Selectable-text and OCR-derived tables with rows, cells, geometry, confidence, provenance, quality signals, and continuation hints. |
| `include_elements` | boolean | on in fast via semantic hints; also on in quality | Structured text, image, and table elements. Semantic hints are returned on these elements, not as a separate field. |
| `include_markdown` | boolean | on in fast | Markdown rendering. |
| `include_html` | boolean | on in quality | HTML rendering. |
| `include_chunks` | boolean | on in fast | Citation-ready chunks. |
| `include_text_layer` | boolean | on in quality | Direction-aware run, line, word, and character evidence with metadata coverage counts. |
| `include_layout_diagnostics` | boolean | on in fast | Reading-order and page-layout confidence. |
| `include_document_map` | boolean | on in fast | Page, element, chunk, OCR, visual candidate, visual enrichment, safety, trust signal-index, accessibility issue-index, and routing map. |
| `include_document_ast` | boolean | on in quality | Semantic AST for page, section, paragraph, list, caption, header, footer, table, image, chart, formula, and figure nodes, including numbered/appendix headings and above/below/side caption evidence links. |
| `include_safety_findings` | boolean | on in balanced and research | Prompt-injection, hidden or near-invisible text geometry, and visual-spoofing findings. |
| `include_trust_report` | boolean | on in balanced and research | Consolidated risk report with page-level signals, category counts, page-risk counts, routing guidance, and optional document-map trust signal routing. |
| `trust_report_redaction` | `"standard" \| "strict" \| "off"` | `"standard"` | Redaction policy for trust-report evidence snippets. `standard` redacts common secrets and personal identifiers, `strict` also redacts phone-like values and IPv4 addresses, and `off` preserves snippets while marking the policy explicitly. |
| `include_accessibility_report` | boolean | on in balanced and research | Tagged-PDF, image-alt, form, permission, tag-visible coverage, issue-summary, page-grade routing, and optional document-map issue-index signals. |
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
bun run build:rust
bun run test:rust          # set ANYMD_CORPUS_DIR to include the Markdown corpus regression
bun run build
bun run package:smoke
bun run test:cov
bun run docs:build
```

`bun run release:preflight` runs the JavaScript side of the same gate.
`package:smoke` packs the package locally and verifies that the tarball ships
only the sole-Rust launcher (`dist/runtime-entry.js`, `dist/pure-rust.js`,
`dist/sdk.js`) with matching `bin` and `exports` metadata, plus the public
corpus and provider-accuracy manifests under `corpus/`.

Release admission is enforced by
`bun scripts/check-verified-candidate-admission.ts`; the publish workflows run
it with `--require-exact-head`.
