# Capability Coverage Matrix

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

Date: 2026-06-14
Status: active

This matrix tracks PDF Reader MCP's shipped and planned capability coverage for
agent-facing PDF intelligence. It intentionally uses neutral capability names
and avoids public comparison language.

## Legend

- Shipped: implemented in this repository.
- In progress: meaningful shipped coverage exists, but the full capability still
  needs fixtures, engine adapters, or additional validation.
- Next: feasible with current architecture and no mandatory heavy dependency.
- Advanced: likely requires optional engines, OCR models, or larger architecture.

## Matrix

| Capability | Status | Notes |
|---|---:|---|
| MCP-native PDF tools | Shipped | V3 exposes `read_pdf`, `search_pdf`, and `pdf_evidence` with stdio/http transport. `pdf_evidence` consolidates inspect, render, crop, OCR, and visual-analysis operations. |
| Agent-native PDF inspection | Shipped | `read_pdf` auto mode profiles PDFs before reading; `pdf_evidence` operation `inspect` is available when callers only need routing metadata. |
| Optional provider readiness | Shipped | Auto reads and `pdf_evidence` operation `inspect` report safe readiness and health metadata for OCR and visual-analysis providers without exposing command paths or arguments; built-in OCR presets become unavailable when their executable is missing. |
| MCP-native PDF search | Shipped | `search_pdf`; bounded literal search with snippets, match offsets, character-derived, text-item, or opt-in OCR-word bounding boxes, and provenance. |
| Local path and URL sources | Shipped | Includes filesystem and HTTP restrictions. |
| Metadata and page count | Shipped | Existing `include_metadata`, `include_page_count`. |
| Text extraction | Shipped | Full text and selected pages. |
| Text layer with run/line/word/char ranges | Shipped | `include_text_layer`; page text, run metadata, direction-aware right-to-left row/column ordering, line IDs, word records, character records, page-level character ranges, estimated char/word boxes, provenance, and metadata coverage diagnostics. |
| Image extraction | Shipped | MCP image parts plus JSON metadata. |
| Visual page rendering | Shipped | `pdf_evidence` operation `render_page`; selected pages render as bounded PNG MCP image parts with evidence metadata and provenance. |
| Region crop evidence | Shipped | `pdf_evidence` operation `extract_regions`; PDF-coordinate bounding boxes crop into focused PNG MCP image parts with evidence metadata. |
| Visual region analysis provider | Shipped | `pdf_evidence` operation `analyze_regions`; focused crops are passed to env-configured command, HTTP, Ollama, OpenAI-compatible, LM Studio, or llama.cpp preset providers and normalized into rich table cell/span/bbox, formula, chart axis/series, figure, image-description, confidence, warning, and provenance fields. No model is bundled. |
| Table extraction | Shipped | Spatial clustering with rows, confidence, and best-effort cell geometry. |
| Table quality diagnostics | Shipped | Completeness, non-empty cell ratio, cell bounding-box coverage, inferred-cell ratio, row alignment, row spacing consistency, missing-cell count, inferred merged-cell candidates, incomplete-geometry warnings, repeated-header continuation candidates, and page-edge geometry continuation candidates. |
| Structured element output | Shipped | `include_elements`. |
| Agent document map | Shipped | `include_document_map`; links pages, elements, selectable text-layer and metadata coverage, chunks, layout diagnostics, safety findings, trust report routing and signal indexes, accessibility report routing and issue indexes, OCR evidence, visual-region candidate indexes, visual enrichment indexes, and page geometry in one agent-ready contract. |
| Semantic document AST | Shipped | `include_document_ast`; page, section, paragraph, list item, caption, header, footer, table, image, chart, formula, figure, diagram, and visual-region nodes linked to element IDs, visual enrichment IDs, chunk IDs, bounding boxes, confidence, section-path context, above/below/side caption-to-evidence links, and table quality metadata. |
| Deterministic semantic hints | Shipped | `include_semantic_hints`; heading, list item, paragraph, caption, header, and footer hints with confidence. Header/footer detection uses page-edge geometry and avoids off-page text. |
| Markdown rendering | Shipped | `include_markdown`. |
| HTML rendering | Shipped | `include_html`; escaped page-aware HTML. |
| Citation-ready chunks | Shipped | `include_chunks`; page, semantic, size, and table strategies with stable element references. |
| Recursive reading order | Shipped | Handles common two-column text segmentation plus spanning-header, independent column-band, and footer ordering, including runtime-generated real PDF coverage for short footers. Needs broader fixture diversity. |
| Layout diagnostics and confidence | Shipped | `include_layout_diagnostics`; page profile, reading-order model, confidence, column signals, and warnings. |
| Outline/bookmark extraction | Shipped | `include_outline`; best-effort when exposed by PDF.js. |
| Annotation extraction | Shipped | `include_annotations`; safe summary fields. |
| Page labels | Shipped | `include_page_labels`. |
| Page geometry | Shipped | `include_page_geometry`; viewport size, rotation, user unit, and view box. |
| Permissions and mark info | Shipped | `include_permissions`. |
| Tagged PDF structure extraction | Shipped | `include_structure_tree`; page-scoped structure trees when exposed by PDF.js. |
| Accessibility report | Shipped | `include_accessibility_report`; deterministic tagged-PDF coverage, tag-to-visible-content coverage, structure tree, heading, image, form, link, permission, mark-info, issue type/severity, document-vs-page issue, page-grade, affected-page signals, and optional document-map issue indexes. Does not claim PDF/UA certification. |
| Form fields | Shipped | `include_form_fields`; covered by a runtime-generated AcroForm fixture with PDF.js zero-based page normalization. Broader form variants still belong in future fixture expansion. |
| Attachment metadata | Shipped | `include_attachments`; metadata only, no attachment bytes by default. |
| Content safety findings | Shipped | `include_safety_findings`; prompt-injection patterns, hidden or near-invisible text geometry, tiny text, off-page text, and overlapping text that may visually spoof or obscure content. |
| PDF trust report | Shipped | `include_trust_report`; consolidates content safety, visual-spoofing, tiny/off-page text, layout uncertainty, sparse/scanned-page, table quality, external-link, and unsafe-link signals with selected-page-scoped category counts, page-risk counts, configurable standard/strict/off evidence redaction, page-level routing guidance, and optional document-map trust signal routing. |
| Rich semantic headings/paragraphs/lists | In progress | Deterministic hints and AST nodes now cover headings, numbered/appendix heading variants, paragraphs, checkbox/bullet/roman list variants, equation/formula and graph/chart caption aliases, headers, footers, cross-page section context, and above/below/side caption-to-evidence linking. Broader multi-caption and multi-target real-world fixtures still need expanded evals. |
| Table cell geometry | Shipped | Table and cell bounding boxes plus row/column indexes where coordinates are available. |
| Rich table spans and multi-page links | In progress | Deterministic header/span hints, repeated-header continuation candidates, and page-edge geometry continuation candidates are shipped; visual provider output can preserve cell spans and boxes; broader arbitrary continuation layouts still need fixture expansion. |
| Semantic chunking | Shipped | Splits chunks on deterministic heading hints when `include_semantic_hints` is enabled. |
| Quality eval and benchmark harness | Shipped | Regression evals cover semantic chunks, table order, table evidence coverage, page-edge table continuation candidates, renderers, safety findings, inspection routing, recursive reading order, visual-region normalization, caption-derived formula/chart/figure candidate routing including side-caption and multi-caption layouts, document-twin visual enrichment fusion, document-map text-layer/metadata/trust-routing/trust-signal-index/accessibility-routing/accessibility-issue-index coverage, above/below/side caption-to-evidence links, search evidence, hidden-text safety routing, selected-page-scoped trust-report category summaries, configurable trust-evidence redaction, visual-spoofing guidance, unsafe-link trust routing, and accessibility issue/page-grade summary routing. `bun run benchmark:quality` publishes deterministic quality gates for Agent Document Twin semantics, inspection tool routing, real PDF document signals, real PDF reading order, deterministic table cell evidence coverage, runtime-generated scanned-PDF OCR pipeline routing, OCR normalization, OCR-derived table extraction, caption-derived visual candidate routing, command/HTTP/Ollama/OpenAI-compatible/LM Studio/llama.cpp visual-region normalization, evidence search, accessibility report summary routing, document-map trust routing, document-map trust signal indexing, document-map accessibility routing, document-map accessibility issue indexing, AI-safety trust-report hidden-text/unsafe-link routing, and a machine-readable SOTA final-bar coverage matrix. `bun run benchmark:corpus` publishes an end-to-end corpus artifact over a checked-in sample plus mandatory runtime-generated reading-order, scanned-OCR routing, and OCR-derived table archetypes, with optional `--corpus-manifest` / `MCP_PDF_CORPUS_MANIFEST` support for operator-supplied path PDFs and public URL PDFs in the same artifact shape; URL cases require SHA256, explicit download opt-in, private-host protection, cache provenance, source metadata, capability tags, and artifact-level capability summaries; the checked-in `corpus/public-url-corpus.json` manifest is included in the package for opt-in public proof without vendoring PDF bytes. `bun run benchmark:provider-manifest-crops` verifies opt-in public PDF provider-manifest downloads, checksums, page renders, and declared region crops without requiring a visual model; `bun run benchmark:provider-manifest` scores configured visual-region providers against the same public PDF crop manifests with SHA256, source metadata, cache provenance, region-level expectations, and capability-summary metadata. `bun run benchmark:providers` reports installed-provider certification profiles, skipped-capability profiles, safe provider-status metadata, provider quality metrics with thresholds and observed evidence, OCR text-layer word boxes, visual table/formula/chart/figure/image-description crop evidence, and a machine-readable final-bar provider evidence matrix when providers are configured. `MCP_PDF_BENCHMARK_OUTPUT_DIR` writes profile-named JSON artifacts for performance, quality, corpus, provider, provider-manifest crop, and provider-manifest scoring reports; `bun run benchmark:release-artifacts` writes deterministic local provider-manifest crop and scoring artifacts without public network or model dependencies; `bun run benchmark:release-gate` blocks release evidence until deterministic quality, required corpus archetypes, deterministic crop-substrate evidence, deterministic provider-manifest scoring evidence, and installed-provider final-bar evidence are complete; `bun run package:smoke` verifies the packed package runtime artifact, package `bin`/`exports` contract, and required public corpus/provider capability manifests. |
| OCR for scanned PDFs | Shipped | `pdf_evidence` operation `ocr_pages`; optional env-configured command provider over bounded rendered pages plus `MCP_PDF_OCR_PRESET=tesseract` and `tesseract-tsv`. `read_pdf` can opt into `include_ocr_text_layer` and link OCR evidence into `document_map`; TSV/provider word boxes are normalized into PDF coordinates and can produce OCR-derived tables when `include_tables` is enabled. No default OCR model is bundled. |
| Formula extraction | Shipped | `pdf_evidence` operation `analyze_regions` can normalize LaTeX, MathML, AsciiMath, text, confidence, and provenance from a configured provider; `read_pdf` can opt into `include_visual_enrichments`, derive bounded formula candidates from caption prefixes even when no image object exists, expose those candidates without a provider, and attach formula evidence to the document twin when a provider is configured. Accuracy depends on the configured local engine. |
| Chart/image descriptions | Shipped | `pdf_evidence` operation `analyze_regions` can normalize chart data points, axes, series, figure, and image-description provider output; `read_pdf` can opt into `include_visual_enrichments`, route table/image elements plus caption-derived chart/figure/diagram candidates, expose those candidates without a provider, and attach visual evidence to the document twin when a provider is configured. Accuracy depends on the configured local engine. |
| Tagged PDF generation | Advanced | Requires separate design and validation. |
| Advanced parser engine adapters | In progress | OCR and visual region provider boundaries, built-in OCR presets, Ollama, OpenAI-compatible, LM Studio, and llama.cpp visual-region presets, provider health metadata, provider benchmark certification profiles, and final-bar provider evidence summaries are shipped; broader accuracy fixtures and optional advanced parser adapters remain. |

## Execution Priority

1. Continue broadening real fixture coverage for the no-new-dependency parity
   features. The quality benchmark now covers outline, annotations, page
   labels, mark info, form fields, attachment metadata, page geometry,
   structure trees, tag-content coverage, semantic variants, and accessibility
   report summary routing through a runtime-generated real PDF, plus real
   multi-column reading order with short footer placement, and the corpus
   benchmark now adds a checked-in sample plus runtime-generated scanned-OCR
   routing and OCR-table archetypes. The checked-in public corpus/provider
   manifests now add CDC statistical chart evidence and public arXiv
   figure/formula/table crops with package-smoke expected-kind gates;
   permissions, additional tagged structures, larger real-world scanned PDFs,
   larger real-world visual-region fixtures, and safety adversarial fixtures
   still need broader coverage.
2. Expand extraction quality evals and benchmarks: multi-column, layout
   diagnostics, tables, annotations, forms, hidden/off-page text, scanned PDFs.
3. Harden the agent document map as the SSOT for pages, elements, text-layer
   coverage, chunks, layout, safety, trust routing, trust signal indexes,
   accessibility routing, accessibility issue indexes, page geometry, and
   optional engine enrichment.
4. Promote run/character evidence into the shared document map and benchmark
   retrieval quality against fixture expectations.
5. Continue broadening the deterministic semantic model beyond shipped
   headings, paragraphs, lists, captions, headers, footers, cross-page section
   context, caption-to-evidence links, AST traversal, and richer table trust
   signals.
6. Continue broadening trust-report adversarial fixtures beyond the shipped
   standard/strict/off redaction policy, visual-spoofing, hidden-text,
   unsafe-link, and selected-page scoping gates.
7. Harden the optional OCR provider with broader scanned fixtures, additional
   provider presets, and accuracy/latency reporting beyond deterministic mock
   provider normalization and the installed Tesseract TSV certification
   benchmark.
8. Harden optional visual region providers for table, formula, chart, figure,
   and image-description engines with broader fixtures, additional presets, and
   accuracy/latency reporting beyond deterministic mock provider normalization,
   the Ollama/OpenAI-compatible/LM Studio/llama.cpp preset request/response
   contracts, and the installed visual-region `visual-full-fidelity`
   certification benchmark.
9. Add formula/chart/tagged-PDF capabilities only through optional engines
   or separately installable modules.

## Public Messaging Rule

Public docs may describe only shipped capabilities. In-progress and advanced
items belong in roadmap language until validated by tests and fixtures.
