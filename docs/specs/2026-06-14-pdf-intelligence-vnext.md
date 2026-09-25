# Full-Fidelity PDF Intelligence Direction

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

Date: 2026-06-14
Status: superseded by shipped release gates and V3 smart tool surface

> V3 note: the public MCP surface is now `read_pdf`, `search_pdf`, and
> `pdf_evidence`; older separate evidence tools remain internal implementation
> boundaries.

## Goal

Make PDF Reader MCP a full-fidelity PDF intelligence layer for AI agents while
preserving its install experience: local-first, MCP-native, simple to run with
`npx`, and safe by default.

The product direction is not to become a monolithic PDF desktop converter. The
product direction is to become the agent-facing control plane for PDF
understanding: extraction, structure, citations, safety, and optional advanced
engines behind one stable MCP contract.

## Non-goals

- Do not make Java, Python, OCR models, or remote services mandatory for the
  default package.
- Do not replace the current `read_pdf` contract in a breaking release.
- Do not claim benchmark superiority until this repository has reproducible
  fixtures and published results.
- Do not implement PDF accessibility remediation before the structured document
  model is stable.
- Do not expose raw parser internals, filesystem paths, or binary payloads in
  JSON summaries.

## Domain Vocabulary

- Source: one local path or URL requested by the MCP caller.
- Document: normalized PDF output for one source.
- Page: one 1-indexed PDF page.
- Element: one structured content object on a page.
- Bounding box: best-effort PDF coordinate rectangle `[left, bottom, right, top]`.
- Engine: the parser backend that produces raw extraction data.
- Renderer: an output adapter that turns normalized document data into JSON,
  Markdown, MCP text parts, or image parts.
- Provenance: metadata describing which engine produced an element and how
  confident the server is.
- Document map: one agent-facing contract that links pages, elements,
  text-layer and metadata coverage, chunks, layout diagnostics, safety
  findings, trust report routing and signal indexes, accessibility report
  routing and issue indexes, visual routing, and page geometry.
- Search evidence: literal text matches with page numbers, snippets, offsets,
  optional character-derived or text-item bounding boxes, and provenance.
- OCR text layer: normalized text, confidence, optional word boxes, language,
  and provenance produced by an explicitly configured local OCR provider.

## Invariants

- Existing `read_pdf` callers keep working without changing arguments.
- The default engine remains local and dependency-light.
- Structured output is additive. It must not force large base64 data into the
  first JSON content part.
- Each public concept has one name across schema, types, docs, tests, and MCP
  responses.
- Every source result must preserve per-source success or failure isolation.
- Unknown parser failures are logged internally but returned to MCP callers as
  curated messages.

## Public Contract

First compatible slice:

```json
{
  "sources": [{ "path": "report.pdf", "pages": "1-3" }],
  "include_elements": true,
  "include_full_text": false,
  "include_metadata": true,
  "include_page_count": true
}
```

When `include_elements` is true, each successful source may include:

```json
{
  "elements": [
    {
      "id": "p1-text-1",
      "type": "text",
      "page": 1,
      "content": "Executive summary",
      "bounding_box": { "left": 72, "bottom": 720, "right": 240, "top": 732 },
      "provenance": { "engine": "pdfjs", "source": "text-content" }
    }
  ]
}
```

Future slices may add `heading`, `paragraph`, `list`, `table`, `image`,
`formula`, `caption`, `header`, and `footer` element types. The first slice
must not overstate semantic detection; text elements remain `text` until a
dedicated classifier exists.

## Architecture

Target boundary:

```text
MCP tool handler
  -> application/use case orchestration
    -> source loading and security policy
    -> engine adapter
    -> normalized document model
    -> renderers
```

Initial implementation can stay in the current modules, but new behavior should
move toward these ports:

```ts
interface PdfEngine {
  readonly name: string;
  parse(source: LoadedPdfSource, options: EngineOptions): Promise<ParsedDocument>;
}

interface PdfRenderer<T> {
  render(document: ParsedDocument, options: RenderOptions): T;
}
```

Candidate engines:

- `pdfjs`: default, local, current dependency set.
- `external-cli`: optional adapter for high-accuracy local parsers.
- `ocr`: optional local provider for scanned PDFs, enabled explicitly outside
  request payloads.
- `vision`: optional enrichment for charts, figures, and formulas.

## Implementation Plan

1. Structured foundation
   - Add automatic `read_pdf` inspection plus `pdf_evidence` operation
     `inspect` for bounded preflight profiling, OCR triage, and ordered
     `next_tools` with recommended `read_pdf` arguments.
   - Add `include_elements`.
   - Add `include_semantic_hints`.
   - Add `include_markdown`.
   - Add `include_html`.
   - Add `include_chunks`.
   - Add `include_text_layer` for run records, line records, word records,
     character records, page-level ranges, estimated bounding boxes, and
     provenance.
   - Add document signals: outline, annotations, page labels, page geometry,
     permissions, structure trees, form fields, and attachment metadata.
   - Add deterministic content safety findings for agent workflows.
   - Add best-effort text and image element metadata.
   - Add best-effort table and table-cell geometry.
   - Add semantic, size, and table-aware chunk strategies.
   - Add layout diagnostics with reading-order confidence for agent routing.
   - Add `include_document_map` as the SSOT response shape for agent
     navigation and future optional engine enrichment.
   - Add `include_document_ast` as a semantic tree over the same element and
     chunk IDs for page/section/paragraph/list/caption/header/footer/table/image
     traversal with continued section context and caption-to-evidence links.
   - Add `include_trust_report` for consolidated content safety,
     visual-spoofing, tiny/off-page text, layout, sparse-page, table quality,
     external-link, unsafe-link, category-count, and page-risk routing
     signals.
   - Add `include_accessibility_report` for deterministic tagged-PDF coverage,
     tag-to-visible-content coverage, structure tree, heading, image, form,
     link, permission, and mark-info signals without claiming PDF/UA
     certification.
   - Add `search_pdf` for bounded evidence retrieval before heavier reading,
     rendering, cropping, or citation workflows.
   - Keep legacy outputs stable.
   - Add tests for schema, JSON response shape, and no binary data in JSON.

2. Layout accuracy
   - Add page geometry and real bounding boxes where available.
   - Add layout-aware reading order beyond simple Y sorting.
   - Split distant same-line text into independent segments, then apply
     conservative recursive band and column segmentation for common
     multi-column PDFs with spanning headers or footers.
   - Add page-level layout profiles, confidence scores, and warnings.
   - Add fixtures for multi-column pages, sidebars, headers, and footers.

3. Semantic extraction
   - Detect headings, paragraphs, lists, captions, headers, footers.
   - Improve tables with quality diagnostics, inferred row/column span hints,
     and multi-page continuation candidates.
   - Add Markdown renderer using the normalized element tree.

4. Safety and trust
   - Detect hidden, tiny, off-page, overlapping, and suspicious invisible text.
   - Broaden sensitive-data redaction fixtures beyond the trust-report evidence
     redaction gate.
   - Add provenance, confidence, warnings, and trace-friendly logs.

5. Advanced engines
   - Add optional provider interface for high-accuracy local engines.
   - Add `pdf_evidence` operation `ocr_pages` as the first optional OCR
     provider interface over bounded rendered pages.
   - Add `pdf_evidence` operation `analyze_regions` for formula, chart, table,
     figure, and image description enrichment behind optional providers over
     bounded crop evidence.
   - Keep the default package lightweight.

6. Agent workflows
   - Add citation-ready chunks with page, semantic, size, table, and bounding
     box references.
   - Add search/index/cache tools for repeated agent work.
   - Add progress and resource telemetry for large PDFs.

7. Accessibility research
   - Audit accessibility report quality across broader tagged and untagged PDF
     fixtures.
   - Evaluate tagged PDF generation only after element model quality is proven.

## Validation Plan

- Unit tests for schema, element construction, page ordering, and binary
  stripping.
- Integration tests for MCP `read_pdf` with and without `include_elements`.
- Integration tests for MCP `read_pdf` with and without `include_document_map`.
- Integration tests for MCP `read_pdf` with and without
  `include_accessibility_report`.
- Integration tests confirm `read_pdf`, `search_pdf`, and `pdf_evidence` are
  exposed by the MCP server, with `pdf_evidence` covering inspect, render, crop,
  OCR, and visual-region analysis operations.
- Quality evals for semantic chunks, table ordering, renderers, and safety
  findings.
- Fixtures for simple text, multi-column reading order, tables, images, scans,
  hidden text, malformed PDFs, and URL security.
- Benchmarks must report accuracy and speed separately.
- Public docs must describe shipped capabilities only; roadmap items must be
  labeled as roadmap.

## Public Messaging Rules

- Talk about product outcomes, not inspirations or external comparisons.
- Do not mention external projects when describing improvements.
- Use neutral language: "structured", "agent-ready", "local-first",
  "citation-ready", "safe by default", "optional advanced engines".
- Do not imply built-in OCR models, formula extraction, chart description, or
  PDF/UA support before they are shipped and validated.
- Prefer measurable claims tied to tests, fixtures, or benchmarks in this repo.

## Acceptance Criteria For First Slice

- `include_elements` validates as an optional boolean.
- `include_semantic_hints` validates as an optional boolean.
- `include_markdown` validates as an optional boolean.
- `include_html` validates as an optional boolean.
- `include_chunks` validates as an optional boolean.
- Document signal flags validate as optional booleans.
- `include_structure_tree` validates as an optional boolean.
- `include_safety_findings` validates as an optional boolean.
- `include_document_map` validates as an optional boolean.
- Requests with `include_elements: true` process selected pages even when
  `include_full_text` is false.
- Requests with `include_semantic_hints: true` return text elements with
  deterministic hints without forcing `full_text`.
- Requests with `include_markdown: true` produce page-aware Markdown without
  forcing `full_text`.
- Requests with `include_html: true` produce escaped page-aware HTML without
  forcing `full_text`.
- Requests with `include_chunks: true` produce chunks with source references,
  strategy labels, and best-effort bounding boxes without forcing `full_text`.
- Requests with `include_chunks: true` and `include_semantic_hints: true`
  split chunks on deterministic heading boundaries when available.
- Requests with `include_page_geometry: true` produce page dimensions and
  rotation without forcing text extraction.
- Requests with `include_structure_tree: true` produce selected-page structure
  trees without forcing text extraction when tagged structure is available.
- Requests with `include_safety_findings: true` produce deterministic findings
  without forcing `full_text`.
- Requests with `include_document_map: true` produce an agent map with pages,
  elements, text-layer and metadata coverage, chunks, layout diagnostics,
  safety findings, trust report routing and signal indexes, accessibility
  report routing and issue indexes, visual routing, page geometry, and summary
  counts without forcing top-level legacy outputs.
- JSON summary includes `elements` with stable ids, page numbers, type, content
  or metadata, and best-effort bounding boxes where available.
- JSON summary does not include base64 image bytes.
- Table output includes row/column-indexed cell metadata, header/span hints,
  inference flags, quality diagnostics, continuation candidates, and
  best-effort bounding boxes when coordinates are available.
- Chunk output includes table chunks when table extraction is requested.
- Document AST output includes page, section, paragraph, list item, caption,
  header, footer, table, and image nodes linked back to element IDs and chunk
  IDs, with section-path context for content that continues across page breaks
  and caption links for nearby matching visual or table evidence.
- Trust report output includes document/page risk levels, risk scores, signals,
  and routing guidance without forcing raw safety, layout, or annotation
  outputs.
- Structure tree output includes sanitized role/type/id/children data only.
- Common two-column text with a full-width title is ordered title, left column,
  then right column.
- Quality evals cover semantic chunks, table ordering, renderer escaping, and
  content safety findings.
- Existing tests for legacy output continue to pass.
- Public README/docs describe structured extraction without mentioning external
  projects.
