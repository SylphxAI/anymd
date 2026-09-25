# Agent Document Map

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

Date: 2026-06-15
Status: shipped

> V3 note: focused visual, OCR, crop, render, and inspection operations are now
> consolidated under `pdf_evidence`; `read_pdf` is the default smart document
> twin entrypoint.

## Goal

PDF Reader MCP exposes a single agent-native map of a PDF: pages,
elements, selectable text-layer and metadata coverage, chunks, layout
confidence, safety findings, trust report routing and signal indexes,
accessibility report routing and issue indexes, OCR evidence, visual evidence
routing, and page geometry. The map is the stable contract that OCR, vision,
formula, chart, accessibility, trust, and advanced table engines can enrich
without forcing agents to learn a new response shape each time.

## Product Positioning

This is the shipped full-fidelity PDF intelligence surface, not a one-feature
parser patch. Public messaging should describe the outcome as agent-ready PDF
understanding with performance-bounded local execution. Do not mention external
projects or imply built-in OCR models, formula, chart, or tagged-PDF generation;
those remain provider-backed or explicitly out of scope unless validated by
dedicated tests and benchmarks.

## Non-Goals

- Do not make OCR, VLMs, ONNX models, Python, Java, or remote services
  mandatory for the default TypeScript package.
- Do not put image base64 data inside the JSON document map.
- Do not create a second table, chunk, or element vocabulary.
- Do not describe provider-backed or advanced parser capabilities as built in.

## Public Contract

`read_pdf` accepts:

```json
{
  "sources": [{ "path": "report.pdf", "pages": "1-5" }],
  "include_document_map": true
}
```

Each successful source may include:

```json
{
  "document_map": {
    "version": "2026-06-15",
    "profile": "agent_document_map",
    "layers": [
      "selectable_text",
      "text_layer",
      "ocr_text_layer",
      "table_structure",
      "semantic_hints",
      "citation_chunks",
      "visual_enrichment",
      "layout_diagnostics",
      "content_safety",
      "trust_report",
      "accessibility_report",
      "page_geometry"
    ],
    "pages": [
      {
        "page": 1,
        "element_ids": ["p1-text-1", "p1-table-1"],
        "chunk_ids": ["p1-chunk-1", "p1-chunk-2"],
        "safety_finding_indexes": [0],
        "text_layer_page_index": 0,
        "text_layer_run_count": 30,
        "text_layer_line_count": 24,
        "text_layer_word_count": 180,
        "text_layer_char_count": 1200,
        "text_layer_runs_with_bounding_boxes": 30,
        "text_layer_lines_with_bounding_boxes": 24,
        "text_layer_words_with_bounding_boxes": 180,
        "text_layer_chars_with_bounding_boxes": 1200,
        "text_layer_runs_with_font_metadata": 30,
        "text_layer_runs_with_direction_metadata": 30,
        "text_layer_runs_with_transform_metadata": 30,
        "text_layer_runs_with_eol_metadata": 30,
        "text_chars": 1200,
        "text_item_count": 30,
        "ocr_text_chars": 128,
        "ocr_word_count": 24,
        "ocr_confidence": 0.91,
        "ocr_source_render_evidence_id": "page-1-render-scale-2",
        "image_count": 0,
        "table_count": 1,
        "visual_enrichment_indexes": [0],
        "visual_enrichment_count": 1,
        "trust_report_page_index": 0,
        "trust_signal_indexes": [0, 1],
        "trust_high_signal_indexes": [0],
        "trust_medium_signal_indexes": [],
        "trust_low_signal_indexes": [1],
        "trust_risk": "medium",
        "trust_score": 42,
        "trust_signal_count": 2,
        "trust_high_signal_count": 1,
        "trust_medium_signal_count": 0,
        "trust_low_signal_count": 1,
        "accessibility_report_page_index": 0,
        "accessibility_issue_indexes": [],
        "accessibility_high_issue_indexes": [],
        "accessibility_medium_issue_indexes": [],
        "accessibility_low_issue_indexes": [],
        "accessibility_grade": "good",
        "accessibility_score": 100,
        "accessibility_issue_count": 0
      }
    ],
    "elements": [],
    "chunks": [],
    "visual_enrichments": [
      {
        "id": "visual-p1-table-1",
        "target_element_id": "p1-table-1",
        "target_element_type": "table",
        "region_id": "p1-table-1",
        "kind": "table",
        "page": 1,
        "provider": "command",
        "source_crop_evidence_id": "page-1-p1-table-1-crop-scale-2",
        "source_bounding_box": {
          "left": 40,
          "bottom": 570,
          "right": 240,
          "top": 602
        }
      }
    ],
    "layout_diagnostics": [],
    "safety_findings": [],
    "routing": {
      "low_confidence_pages": [],
      "image_or_sparse_pages": [],
      "needs_ocr_pages": [],
      "ocr_applied_pages": [1],
      "trust_review_pages": [1],
      "trust_high_signal_pages": [1],
      "trust_high_risk_pages": [],
      "trust_medium_risk_pages": [1],
      "accessibility_review_pages": [],
      "accessibility_high_issue_pages": [],
      "accessibility_medium_issue_pages": [],
      "accessibility_low_issue_pages": []
    },
    "summary": {
      "selected_pages": [1],
      "processed_page_count": 1,
      "element_count": 0,
      "text_element_count": 0,
      "text_layer_page_count": 1,
      "text_layer_run_count": 30,
      "text_layer_line_count": 24,
      "text_layer_word_count": 180,
      "text_layer_char_count": 1200,
      "text_layer_runs_with_bounding_boxes": 30,
      "text_layer_lines_with_bounding_boxes": 24,
      "text_layer_words_with_bounding_boxes": 180,
      "text_layer_chars_with_bounding_boxes": 1200,
      "text_layer_runs_with_font_metadata": 30,
      "text_layer_runs_with_direction_metadata": 30,
      "text_layer_runs_with_transform_metadata": 30,
      "text_layer_runs_with_eol_metadata": 30,
      "ocr_page_count": 1,
      "ocr_text_chars": 128,
      "image_element_count": 0,
      "table_element_count": 0,
      "visual_enrichment_count": 1,
      "visual_enrichment_kind_counts": { "table": 1 },
      "chunk_count": 0,
      "safety_finding_count": 0,
      "trust_report_page_count": 1,
      "trust_risk": "medium",
      "trust_score": 42,
      "trust_signal_count": 2,
      "trust_high_signal_count": 1,
      "trust_medium_signal_count": 0,
      "trust_low_signal_count": 1,
      "trust_pages_with_signals": 1,
      "trust_high_risk_page_count": 0,
      "trust_medium_risk_page_count": 1,
      "trust_signal_type_counts": { "content_safety": 1, "external_link": 1 },
      "accessibility_report_page_count": 1,
      "accessibility_score": 100,
      "accessibility_grade": "good",
      "accessibility_issue_count": 0,
      "accessibility_pages_with_issues_count": 0,
      "accessibility_page_grade_counts": { "good": 1, "partial": 0, "weak": 0 }
    }
  }
}
```

## Invariants

- `document_map.pages[*].element_ids` must reference IDs present in
  `document_map.elements`.
- `document_map.pages[*].chunk_ids` must reference IDs present in
  `document_map.chunks`.
- `document_map.pages[*].safety_finding_indexes` must reference array indexes
  in `document_map.safety_findings`.
- `document_map.pages[*].visual_enrichment_indexes` must reference array
  indexes in `document_map.visual_enrichments`.
- `document_map.pages[*].text_layer_page_index` must reference the same-page
  record in the internally built or top-level `text_layer.pages` array.
- `document_map.pages[*].trust_report_page_index`, when present, must
  reference the same-page record in the internally built or top-level
  `trust_report.page_reports` array.
- `document_map.pages[*].accessibility_report_page_index`, when present, must
  reference the same-page record in the internally built or top-level
  `accessibility_report.page_reports` array.
- `document_map.visual_enrichments[*].target_element_id` must reference either
  the table/image element whose crop was analyzed or a stable synthetic
  caption-derived visual region ID. Caption-derived regions must include
  `source_caption_element_id` so agents can trace the crop back to text
  evidence.
- `document_map.layers` must be derived from actual emitted layers, not user
  flags. `text_layer` is present only when selectable text-layer evidence was
  built. `ocr_text_layer` is present only when OCR pages were returned.
- Top-level legacy outputs remain opt-in. `include_document_map` may build
  internal elements, chunks, text-layer and metadata coverage, safety findings,
  trust routing summaries, accessibility routing summaries, accessibility
  issue indexes, layout diagnostics, page geometry, and tables for the map
  without forcing top-level `elements`, `chunks`, `text_layer`,
  `safety_findings`, `layout_diagnostics`, `page_geometry`, `annotations`,
  `tables`, or raw tagged-structure outputs.
- JSON output must not include embedded image bytes.
- Visual enrichment is opt-in and provider-backed. If the provider is not
  configured or fails, `read_pdf` records a warning instead of failing the
  whole document read.
- Every map is deterministic for the same PDF bytes, selected pages, and parser
  version.

## Performance Constraints

- The map must reuse page content, table, layout, safety, chunk, and geometry
  data already produced in the request path. Text-layer coverage must be
  derived from the same text-layer builder used by `include_text_layer`.
- The builder itself must be linear in emitted pages, elements, chunks, and
  findings.
- Page extraction remains batched; document map construction must not increase
  source concurrency beyond the existing handler limit.
- OCR, VLM, and ONNX work belongs behind optional provider interfaces, with
  health checks and explicit enablement. `include_ocr_text_layer` is the
  explicit opt-in for OCR fusion and must keep OCR text separate from
  selectable text.
- `include_visual_enrichments` is the explicit opt-in for visual-region
  provider fusion. It must respect `max_visual_enrichments`, avoid embedding
  crop image bytes in JSON, and attach crop evidence IDs to the document twin.
- Visual-region candidates are first-class routing evidence. When visual
  enrichment is requested, the map must preserve candidate indexes, target
  types, PDF-coordinate boxes, caption provenance, and routing signals even if
  the optional provider is unavailable.
- Accessibility report summaries are first-class routing evidence. When
  `include_accessibility_report` and `include_document_map` are both enabled,
  the map must link page report indexes, issue indexes, per-page issue counts,
  affected-page routing arrays, and document-level accessibility summary counts
  without forcing raw structure trees into top-level output.
- Trust report summaries are first-class routing evidence. When
  `include_trust_report` and `include_document_map` are both enabled, the map
  must link page report indexes, signal indexes, page risk, score, signal
  counts, high-signal routing arrays, high/medium-risk routing arrays, and
  document-level trust summary counts without forcing raw safety, layout,
  annotation, or table outputs into top-level output.

## Capability Batch

The shipped capability batch includes:

- `include_document_map` public schema, handler, types, tests, docs.
- Inspector recommendations include `include_document_map` for digital and
  mixed PDFs.
- `pdf_evidence` operations provide visual evidence, focused crops, visual
  region enrichment, and configured OCR text layers without embedding image
  bytes in JSON summaries.
- `read_pdf` can opt into OCR text layer fusion for sparse/scanned pages and
  link applied OCR pages into the document map.
- `include_document_map` links selectable text-layer coverage into page records
  and summary totals, including run-level font, direction, transform, and
  end-of-line metadata coverage, without forcing the top-level `text_layer`
  response.
- `include_document_map` links accessibility report routing summaries when
  `include_accessibility_report` is also requested, without forcing raw
  structure-tree output. Page records include issue indexes for direct evidence
  routing into `accessibility_report.issues`.
- `include_document_map` links trust report routing summaries when
  `include_trust_report` is also requested, including signal indexes for direct
  evidence routing, without forcing raw safety, layout, annotation, or table
  outputs.
- `read_pdf` can opt into visual enrichment fusion for bounded table/image
  regions and caption-derived formula/chart/figure/diagram regions, then link
  provider-backed table, formula, chart, figure, diagram, or image evidence
  into the document map and AST.
- `search_pdf` provides bounded evidence retrieval with snippets, offsets,
  optional character-derived or text-item bounding boxes, and provenance before
  heavier workflows.
- Quality eval proves the map links pages, elements, text-layer and metadata
  coverage, chunks, safety findings, trust report routing and signal indexes,
  layout diagnostics, and geometry.
- Handler tests prove the map does not force top-level legacy outputs.
- Public docs describe shipped configured OCR and configured visual enrichment
  accurately, and keep built-in OCR model, bundled VLM, and PDF/UA generation
  capabilities in roadmap language only.

Next evidence-expansion slices:

- Additional OCR provider presets, scanned fixtures, and accuracy/latency
  benchmarks.
- Additional engine-specific layout, table, formula, and chart provider presets
  plus accuracy/latency fixtures beyond the shipped Ollama and
  OpenAI-compatible request/response presets.
- Benchmark harness with accuracy and latency reported separately.
- Broader fixtures for multi-column, forms, tables, scanned pages, hidden text,
  charts, and formulas.

## Acceptance Criteria

- `include_document_map` validates as an optional boolean.
- `read_pdf` with `include_document_map: true` processes selected pages even
  when `include_full_text` is false.
- The map includes semantic elements, selectable text-layer and metadata
  coverage, citation chunks, layout diagnostics, safety findings,
  trust report routing and signal indexes, accessibility report routing and
  issue indexes, visual routing, page geometry, and summary counts.
- The map includes table elements when deterministic table extraction finds
  tables.
- The map links OCR pages when `include_ocr_text_layer` returns OCR evidence,
  and links OCR-derived table elements when OCR word boxes form table
  structure.
- The map links visual enrichment evidence when `include_visual_enrichments`
  returns provider-backed region analysis.
- The map links accessibility report page indexes, issue counts, affected-page
  routing arrays, issue indexes, and grade summaries when
  `include_accessibility_report` is also enabled.
- The map links trust report page indexes, risk, scores, signal counts,
  signal indexes, high-signal routing arrays, high/medium-risk routing arrays,
  and signal summaries when
  `include_trust_report` is also enabled.
- The AST attaches visual enrichment evidence to semantic nodes without
  duplicating table elements.
- The AST links captions to nearby matching table, image, figure, chart,
  formula, or diagram evidence without moving page-local nodes.
- The first JSON content part omits `page_contents` and image bytes.
- Existing `read_pdf` calls without `include_document_map` remain unchanged.
- Automatic `read_pdf` inspection and `pdf_evidence` operation `inspect`
  recommend the document map for agentic digital-text and mixed workflows.
- Full validation passes before merge.
