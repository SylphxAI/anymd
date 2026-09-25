# SOTA Final Bar

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

Date: 2026-06-15
Status: active

## Goal

PDF Reader MCP is complete only when it is a TypeScript-first full-fidelity PDF
intelligence system for agents, not merely a PDF parser with optional helpers.
The target product shape is an Agent Document Twin: every agent-facing answer
must be traceable to text, visual, semantic, safety, and evidence layers.

This document is the completion bar for future release decisions. Foundation
milestones, adapter interfaces, or roadmap wording do not satisfy this bar by
themselves.

## Required Capabilities

1. Lossless selectable-text evidence
   - Preserve text runs, font/direction metadata where exposed, line records,
     word records, character records, page offsets, and provenance.
   - Use exposed text direction for right-to-left row and same-direction column
     ordering instead of forcing all selectable text into left-to-right order.
   - Report run-level font, direction, transform, and end-of-line metadata
     coverage in text-layer and document-map summaries.
   - Provide bounding boxes at run, line, word, character, table, cell, image,
     crop, and page levels when the source exposes enough geometry.
   - Label estimated geometry explicitly; never imply glyph-perfect geometry
     unless backed by a source that proves it.

2. Robust reading order
   - Implement deterministic recursive layout segmentation for mixed
     single-column, multi-column, spanning-header, footer, sidebar, and table
     regions.
   - Expose reading-order confidence and diagnostics that route agents to
     visual verification when confidence is low.

3. Scanned-PDF pipeline
   - Render selected pages, run OCR through a validated local provider or
     separately installable preset, normalize word boxes, and fuse OCR output
     into the document map with provenance.
   - Include scanned fixtures and accuracy/latency reporting.

4. Table intelligence
   - Combine deterministic text-table clustering with visual table recognizer
     provider support.
   - Report cell boxes, header/span hints, continuation candidates, confidence,
     and failure modes against public fixtures.

5. Formula, chart, and figure enrichment
   - Support provider-backed formula recognition, chart-to-data extraction, and
     figure/image descriptions.
   - Normalize outputs into the same evidence model with crop IDs,
     confidences, warnings, caption-derived visual-region routing, and
     benchmark coverage.

6. Tagged-PDF and accessibility intelligence
   - Read structure trees when available, correlate tags with visible content,
     surface accessibility risks, summarize issue types, severities,
     document-vs-page issue totals, page-grade buckets, and affected-page
     counts for agent routing, and avoid claiming certification unless a
     dedicated validator proves it.

7. AI-safety trust reporting
   - Detect hidden, tiny, off-page, overlapping, visually spoofed, unsafe-link,
     and prompt-injection-like content with page evidence and routing guidance.
   - Summarize selected-page-scoped trust evidence by signal type, safety
     finding type, severity, and page-risk bucket so agents can route high-risk
     PDFs without scanning every raw signal first.
   - Redact common sensitive values from trust-report evidence snippets while
     marking the redaction types, expose the selected redaction policy, and
     support stricter phone/IP redaction for higher-sensitivity local runs so
     routing metadata does not unnecessarily expose secrets.

8. Reproducible proof
   - Public benchmark commands must measure speed and quality, not just runtime.
   - Benchmark commands must be able to persist machine-readable JSON artifacts
     for release review.
   - Fixtures must cover selectable text, multi-column layout, tables, scans,
     formulas, charts, figures, hidden text, annotations, forms, attachments,
     and tagged PDFs.
   - `bun run benchmark:quality` is the deterministic contract-quality gate for
     Agent Document Twin semantics, inspection tool routing, real PDF document
     signals, real PDF reading order, text-layer evidence and metadata coverage in the
     document map, document-map trust routing, document-map trust signal
     indexing, document-map accessibility routing, document-map accessibility
     issue indexing,
     accessibility tag-to-visible-content coverage, accessibility
     issue/page-grade summaries, table cell evidence coverage,
     caption-to-evidence links, OCR
     normalization, a runtime-generated scanned-PDF OCR pipeline fixture,
     OCR-derived table extraction from scanned-page word boxes,
     caption-derived visual candidate routing,
     visual-region command/HTTP/Ollama/OpenAI-compatible/LM Studio/llama.cpp normalization,
     search evidence, and AI-safety trust-report
     selected-page-scoped category summaries, configurable trust-evidence
     redaction, visual-spoofing guidance, hidden-text routing, and unsafe-link
     routing.
     Its JSON report includes `final_bar_coverage_summary` and
     `final_bar_coverage`, mapping each SOTA final-bar capability to the
     deterministic benchmark scenarios that currently prove it and marking
     areas that still require installed-provider benchmark evidence.
   - `bun run benchmark:providers` is the installed-provider benchmark
     for optional OCR engines such as `tesseract-tsv` and configured
     visual-region providers. It reports an `ocr-text-layer` profile for OCR
     word-box fusion and a `visual-full-fidelity` profile for runtime table,
     formula, chart, figure, and image-description crop certification when
     those providers are installed. It also emits safe provider-status metadata
     and skipped-capability certification profiles when providers are absent, so
     release gates can distinguish a missing optional engine from a failed
     engine. Skipped providers are explicit and can be made blocking with
     `MCP_PDF_PROVIDER_BENCHMARK_REQUIRED=true`. Its JSON report includes
     per-provider `quality` metrics with thresholds, scores, expected evidence,
     observed evidence, `final_bar_provider_evidence_summary`, and
     `final_bar_provider_evidence`, mapping installed-provider certification
     profiles to the final-bar capabilities that still require provider-backed
     evidence.
   - Provider-specific OCR, table, formula, chart, and image-description
     accuracy beyond the certification fixtures still requires public
     scanned/visual fixture benchmarks before any model-quality claim.
   - `MCP_PDF_BENCHMARK_OUTPUT_DIR=./benchmark-artifacts MCP_PDF_PROVIDER_BENCHMARK_REQUIRED=true bun run benchmark:all`
     writes profile-named release artifacts for performance, deterministic
     quality, corpus, and strict installed-provider evidence reports.
     `benchmark:release-artifacts` also writes a deterministic
     `pdf_provider_manifest_crop_benchmark.json` artifact over a local crop
     fixture manifest so release evidence proves page rendering, region crop
     geometry, crop metadata, render provenance, and crop capability summaries
     without network access, OCR, a visual provider, or a local model.
     It also writes a deterministic `pdf_provider_manifest_benchmark.json`
     artifact over local table, formula, chart, figure, and image regions so
     release evidence proves manifest-driven provider scoring, normalized
     visual kinds, text/confidence assertions, crop provenance, and capability
     summaries without public network access.
     Release automation installs Tesseract for the OCR profile and configures
     the repository reference visual provider for the runtime visual fixture
     profile before running this gate.
   - `bun scripts/benchmark-pdf-corpus.ts --corpus-manifest ./corpus-manifest.json`
     or `MCP_PDF_CORPUS_MANIFEST=./corpus-manifest.json bun run benchmark:corpus`
     can add operator-supplied real PDFs to the corpus artifact using the same
     assertion format. Corpus cases may include `capability_tags`; the artifact
     emits `capability_summary` so release evidence can be reviewed by coverage
     area. Release CI remains deterministic and does not download or bundle
     external PDFs by default.
   - Public URL corpus cases must include `sha256`. They resolve through a
     content-addressed cache and require `--allow-corpus-downloads` or
     `MCP_PDF_CORPUS_ALLOW_DOWNLOADS=true` before the benchmark performs a
     network fetch. Cached bytes are revalidated against the checksum before
     parsing, private/loopback/link-local hosts remain blocked unless the
     existing private-IP development override is enabled, and URL/cache
     provenance is recorded in the corpus artifact.
   - `corpus/public-url-corpus.json` is included in the repo and published
     package as an opt-in public corpus manifest over official and publicly
     available PDF sources with source metadata, pinned SHA256 values, and
     required capability tags covering forms, accessibility guidance, scans,
     statistical reports, research papers, charts, formulas, and tables. It is
     not part of default CI network activity.
   - `corpus/public-provider-accuracy.json` is included in the repo and
     published package as an opt-in public provider accuracy manifest over
     official and publicly available PDF crops with expected-kind coverage for
     chart, diagram, figure, formula, image, and table evidence.
     `benchmark:provider-manifest-crops` verifies public PDF downloads,
     SHA256 values, page renders, and declared region crops without requiring
     a visual-region provider or local model. `benchmark:provider-manifest`
     can then score a configured visual-region provider against those crop
     expectations and emits capability-summary metadata without making default
     CI depend on network access or external model availability.
   - `MCP_PDF_BENCHMARK_OUTPUT_DIR=./benchmark-artifacts bun run benchmark:release-gate`
     reads those artifacts and must pass before a SOTA release can be treated
     as complete. It fails if deterministic final-bar coverage is incomplete,
     if mandatory corpus archetype evidence or required corpus capability
     summaries are incomplete, if deterministic provider-manifest crop evidence
     or required crop capability summaries are missing or incomplete, if
     deterministic provider-manifest scoring evidence, required visual-kind
     coverage, or scoring capability summaries are missing or incomplete, if
     any quality area still needs provider-backed evidence, or if the provider
     artifact was not produced with strict provider requirements. It also fails
     when provider quality metrics are missing or not passing for an
     installed-provider certification result.

9. Public contract integrity
   - README, docs, changelog, release notes, and package metadata may describe
     only validated shipped behavior.
   - Advanced capabilities remain roadmap language until tests, evals, and
     benchmarks prove them.

## Release Gate

A new major release should not be treated as complete until:

- Every required capability above has direct test, fixture, eval, or benchmark
  evidence.
- CI runs deterministic checks and the non-publishing strict release-evidence
  gate, or a documented local benchmark command produces a reproducible
  artifact when local provider tools are intentionally absent.
- `bun run benchmark:release-gate` passes against the release benchmark
  artifacts.
- `bun run package:smoke` passes against the packed package tarball.
- Public docs match the verified behavior without external-project callouts or
  unproven superiority claims.
- The package smoke test proves the release tarball exposes the executable
  runtime artifact, the expected `bin`/`exports` contract, required public
  corpus/provider capability coverage manifests, corpus assertion/read-option
  contracts, and public provider bbox/expected-kind/normalized-confidence/text
  contracts.
