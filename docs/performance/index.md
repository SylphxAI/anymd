# Performance

PDF Reader MCP is optimized for **agent workflow latency**, bounded local
execution, and reproducible release evidence.

## Product performance (4.1.x)

On controlled same-host **linux-x64** dual-mode A/B vs historical TS `3.0.14`,
using registry-installed **4.1.0/4.1.1** natives:

| Mode | Result |
| --- | --- |
| `persistent_warm` (long-lived server, repeated identical local reads) | **≥ ~10×** on 8 required fixture classes |
| `startup_inclusive` (spawn + initialize + task) | large advantage |

Method bounds apply: first request still pays full parse cost; warm path may use
process-local identical-request cache; not a multi-host guarantee.

See [Product proof](/guide/product-proof) and the
[4.1.0 report](https://github.com/SylphxAI/citra/blob/main/docs/specs/performance/4.1.0-same-host-performance-report.md).

---

Quality claims remain gated through deterministic fixtures, corpus cases,
provider certification, package smoke checks, and the SOTA release gate.

## Release Proof Snapshot

The checked-in release artifacts expose machine-readable proof for the current
capability surface:

| Artifact | Current release evidence |
| --- | --- |
| `pdf_sota_release_gate.json` | `passed`, 39/39 release-gate checks passing |
| `pdf_quality_benchmark.json` | score `1`, 69/69 deterministic quality checks passing |
| `pdf_provider_benchmark.json` | strict provider evidence enabled, 4/4 final-bar provider profiles certified |
| `pdf_corpus_benchmark.json` | corpus-style PDF intelligence assertions with capability summaries |
| `pdf_provider_manifest_crop_benchmark.json` | deterministic crop-substrate proof for provider-manifest regions |
| `pdf_provider_manifest_benchmark.json` | deterministic provider-manifest scoring proof for table, formula, chart, figure, and image regions |

## Reproducible Benchmark

Run the local benchmark against the checked-in sample PDF:

```bash
bun run benchmark
```

The benchmark performs warmup iterations, then prints a table and JSON summary
with average, minimum, and maximum latency for these fixed scenarios:

| Scenario | Notes |
|----------|-------|
| `metadata_page_count` | Fast metadata and page-count path |
| `full_text` | Full selectable-text extraction |
| `selected_page_text` | Single-page extraction |
| `v3_agent_document_twin` | Agent Document Twin scenario: document map, text layer, document AST, trust report, accessibility report, chunks, semantic hints, layout diagnostics, tables, and trust/accessibility routing plus index fusion |

Treat benchmark output as machine- and fixture-specific. Public performance
claims should cite the command, fixture, runtime, and measured output.

## Benchmark Artifacts

All benchmark scripts print JSON to stdout and can also write formatted JSON
artifacts for release review:

```bash
MCP_PDF_BENCHMARK_OUTPUT_DIR=./benchmark-artifacts bun run benchmark:all
```

Release artifacts should be produced with provider requirements enabled:

```bash
MCP_PDF_BENCHMARK_OUTPUT_DIR=./benchmark-artifacts MCP_PDF_PROVIDER_BENCHMARK_REQUIRED=true bun run benchmark:all
```

`benchmark:all` writes one artifact per report profile:
`pdf_performance_benchmark.json`, `pdf_quality_benchmark.json`,
`pdf_corpus_benchmark.json`, and `pdf_provider_benchmark.json`.
`benchmark:release-artifacts` adds the deterministic
`pdf_provider_manifest_crop_benchmark.json` release artifact over a local crop
manifest so the crop substrate is gated without network access or a local
model. It also adds a deterministic `pdf_provider_manifest_benchmark.json`
artifact over local table, formula, chart, figure, and image regions so the
provider-manifest scoring path is gated without public network access.
Individual benchmark scripts also accept
`--output <path>` for a single report file or `--output-dir <dir>` for a
profile-named report file.

Run the release gate after writing artifacts:

```bash
MCP_PDF_BENCHMARK_OUTPUT_DIR=./benchmark-artifacts bun run benchmark:release-gate
```

The release gate writes `pdf_sota_release_gate.json` when artifact output is
enabled. It exits non-zero until deterministic quality coverage is complete,
the corpus benchmark is fully passing with checked-in and runtime-generated
fixture diversity, all quality areas that require installed-provider evidence
are certified by `benchmark:providers`, and the provider benchmark artifact was
produced with strict provider requirements enabled. It also requires the
deterministic provider-manifest crop artifact to include passing case, region,
crop metadata, and required capability-summary evidence, and the deterministic
provider-manifest scoring artifact to include passing table, formula, chart,
figure, image, kind, confidence, text, crop-provenance, and required
capability-summary evidence. Provider quality metrics must be present and
passing for installed-provider certification results, so release review is not
based only on a single aggregate score.

## Quality Benchmark

Run the deterministic quality benchmark:

```bash
bun run benchmark:quality
```

The quality benchmark prints a table and JSON report. It exits with a non-zero
status if any quality gate fails. The JSON report also includes
`final_bar_coverage_summary` and `final_bar_coverage`, a machine-readable map
from the SOTA final-bar capabilities to the benchmark scenarios that prove
deterministic coverage. Entries marked `provider_benchmark_required` have
passing deterministic coverage but still require installed-provider benchmark
evidence before making engine-specific accuracy claims.

| Scenario | Quality gate |
|----------|--------------|
| `agent_document_twin_semantic_quality` | Semantic roles for headings, lists, paragraphs, captions, headers, and footers; numbered/appendix heading variants; checkbox/bullet list variants; equation/formula and graph/chart caption aliases; side-caption visual-region routing; multi-caption and multi-target visual-region routing; cross-page section context; document-map text-layer, metadata, trust-routing, trust-signal-index, accessibility-routing, and accessibility-issue-index coverage; above/below/side caption-to-evidence links; citation chunks; table ordering; safety findings; Markdown/HTML rendering; direction-aware text-layer evidence; document map; document AST; accessibility report tag-content coverage plus issue/page-grade summary routing; and inspection tool routing |
| `document_signal_fixture_quality` | Runtime-generated real PDF fixture through `read_pdf` for outline, page labels, mark info, link and widget annotations, AcroForm fields, embedded attachment metadata, page geometry, tagged structure tree roles/content references, and accessibility report fusion with routeable issue/page-grade summaries |
| `real_reading_order_fixture_quality` | Runtime-generated real multi-column PDF through `read_pdf` for spanning headers, independently ordered columns, short footer placement, text-layer line order, and mixed-layout diagnostics |
| `recursive_reading_order_quality` | Spanning header, independent column bands, and footer reading sequence |
| `ocr_text_layer_quality` | Local OCR provider normalization, word boxes, confidence, language, render evidence, and OCR text-layer summary |
| `scanned_pdf_fixture_pipeline_quality` | Runtime-generated image-only PDF fixture through `read_pdf` load, render, OCR provider, OCR text-layer fusion, document map routing, and low-confidence layout diagnostics |
| `ocr_table_extraction_quality` | Runtime-generated scanned PDF through render, OCR word-box normalization, OCR-derived table extraction, document-map table fusion, and document AST table provenance |
| `visual_region_analysis_quality` | Local command, HTTP, Ollama-preset, OpenAI-compatible, LM Studio, and llama.cpp visual-region provider normalization for table cells/spans/boxes, formula fields, chart axes/series, figure and image-description evidence, confidence, warnings, request shape, and crop evidence |
| `search_evidence_quality` | Selectable text search with character-derived boxes and OCR search with word-level boxes plus render provenance |
| `table_evidence_quality` | Deterministic table cell bounding-box coverage, inferred-cell ratios, weak-geometry routing warnings, and page-edge continuation candidates |
| `ai_safety_trust_report_quality` | Hidden or near-invisible text geometry, overlapping text detection for visual-spoofing or obscured-content risk, selected-page-scoped trust-report signal/safety category counts, page-risk counts, configurable trust-evidence redaction, visual-spoofing guidance, and unsafe-link scheme routing |

This benchmark uses in-repository synthetic cases, runtime-generated
document-signal, reading-order, and scanned PDF fixtures, and mock local
providers so it is reproducible in CI and on developer machines. It is a
contract-quality gate, not a claim about a particular OCR, table, formula,
chart, figure, image-description, or vision model's real-world accuracy.
Provider-specific accuracy and latency claims require separate public
scanned/visual fixture runs.

## Corpus Benchmark

Run the corpus benchmark when you want a compact public proof artifact that is
closer to end-to-end agent use than isolated unit fixtures:

```bash
bun run benchmark:corpus
```

The corpus benchmark covers a checked-in text-rich PDF plus mandatory
runtime-generated multi-column reading-order, scanned-page OCR routing, and
OCR-derived table recovery archetypes. Each case reports fixture type,
document archetype, capability tags, metrics, expected evidence, observed
evidence, and assertion-level pass/fail status. The JSON artifact also emits a
`capability_summary` so reviewers can inspect coverage by document-intelligence
area instead of relying only on a single aggregate score. Release gates require
the exact archetype case set, checked-in and runtime-generated fixture
diversity, case-level capability tags, capability-summary coverage for
required corpus areas, and per-case passing assertion evidence in addition to
performance, deterministic quality, and installed-provider evidence.

Teams can extend the same artifact with real PDFs they are licensed to use by
passing an external manifest. The built-in release gate does not depend on
network downloads or bundled external PDFs, but the manifest mode lets release
reviewers compare scanned, visual, domain-specific, or customer-like fixtures
with the same assertion format:

```bash
bun scripts/benchmark-pdf-corpus.ts --corpus-manifest ./corpus-manifest.json
```

```json
{
  "cases": [
    {
      "id": "agency-scan",
      "path": "./fixtures/agency-scan.pdf",
      "pages": [1],
      "document_archetype": "external scanned form",
      "capability_tags": ["external_scan", "ocr_text_layer"],
      "expected": {
        "min_pages": 1,
        "min_ocr_words": 20,
        "required_document_map_layers": ["ocr_text_layer", "page_geometry"]
      },
      "read_pdf_options": {
        "include_ocr_text_layer": true,
        "include_document_map": true
      }
    }
  ]
}
```

Manifest cases can use either a local `path` or a public `url`. URL cases must
include a 64-character `sha256`; the benchmark uses a content-addressed cache,
verifies the cached or downloaded bytes before parsing, and downloads only when
explicitly enabled:

```bash
MCP_PDF_CORPUS_ALLOW_DOWNLOADS=true \
  bun scripts/benchmark-pdf-corpus.ts \
  --corpus-manifest ./corpus/public-url-corpus.json \
  --corpus-cache-dir ./.cache/pdf-corpus
```

```json
{
  "cases": [
    {
      "id": "public-report",
      "url": "https://example.org/public-report.pdf",
      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "document_archetype": "public benchmark report",
      "capability_tags": ["public_report", "selectable_text"],
      "expected": {
        "min_pages": 1,
        "min_text_chars": 500
      }
    }
  ]
}
```

After the first verified download, the same manifest can run from cache without
network access. The report records URL case count, actual download count, cache
directory, source type, URL, checksum, source metadata, and whether each URL
case used a fresh download or cached bytes. Private, loopback, and link-local
URL hosts are blocked by default; local fixture servers require the existing
`--allow-private-ips` or `MCP_PDF_ALLOW_PRIVATE_IPS=true` override.

The repository includes `corpus/public-url-corpus.json`, an opt-in manifest of
official and publicly available PDFs with pinned SHA256 values and capability
tags for forms, accessibility guidance, public-domain text, technical reports,
and legacy image-plus-text documents. It is included in the published package
so release reviewers and downstream users can reproduce a real-world public
corpus artifact without vendoring PDF bytes or making default CI depend on
network access. `bun run package:smoke` verifies that the packed package keeps
the required public corpus/provider capability tag coverage, corpus expected
assertions and read options, provider-region bbox/normalized-confidence/text
contracts, and provider expected-kind coverage for chart, diagram, figure,
formula, image, and table regions.

## Provider Benchmark

Run the optional installed-provider benchmark when the local machine has OCR or
visual-region providers installed:

```bash
bun run benchmark:providers
```

The provider benchmark exercises the `tesseract-tsv` OCR preset over multiple
runtime-generated PDFs rendered through `read_pdf` OCR fusion, and it can
exercise a configured visual-region command or HTTP provider over 10
runtime-generated table, formula, chart, figure, and image-description visual
fixture regions. The benchmark crops those regions through the same rendering
path as `pdf_evidence` operation `analyze_regions` and reports a
`visual-full-fidelity` certification profile covering crop provenance, table
cell boxes, formula formats, chart
axes or series, figure descriptions, and image-description text.

The repository includes `scripts/reference-region-analysis-provider.mjs` as a
deterministic command provider for the visual certification fixtures. It is
useful for release evidence and contract regression checks, but it is not a
general-purpose vision model. OCR certification still requires an installed
Tesseract executable because the benchmark verifies the real `tesseract-tsv`
preset and OCR document-map fusion path.

The JSON report includes `certification_profiles`, safe `provider_status`
metadata, per-provider `certification` summaries, per-provider `quality`
metrics with thresholds, scores, expected evidence, and observed evidence,
`final_bar_provider_evidence_summary`, and `final_bar_provider_evidence` so
release environments can distinguish installed provider smoke checks, missing
optional engines, and provider-backed final-bar evidence.

Unavailable providers report `skipped` and still emit certification profiles
with skipped capabilities by default. This keeps the JSON contract stable
whether a developer machine has optional engines installed or not.
Release or provider-certification environments can make skipped providers fail
with:

```bash
MCP_PDF_PROVIDER_BENCHMARK_REQUIRED=true bun run benchmark:providers
```

CI and release workflows install Tesseract, configure the reference visual
provider, write strict benchmark artifacts, and then run
`benchmark:release-gate` without publishing from the CI evidence job.

Run the opt-in public provider accuracy manifest when a local visual-region
provider is configured and you want real public PDF crop evidence:

```bash
MCP_PDF_PROVIDER_MANIFEST_ALLOW_DOWNLOADS=true \
  bun run benchmark:provider-manifest-crops \
  --provider-manifest ./corpus/public-provider-accuracy.json \
  --provider-manifest-cache-dir ./.cache/pdf-corpus
```

That command verifies public PDF downloads, SHA256 checksums, page renders, and
declared region crops without requiring OCR, a local vision model, or a visual
provider. Use the provider-scoring benchmark when the crop substrate is proven
and a visual-region provider is configured:

```bash
MCP_PDF_PROVIDER_MANIFEST_ALLOW_DOWNLOADS=true \
  MCP_PDF_REGION_ANALYSIS_PRESET=ollama \
  MCP_PDF_REGION_ANALYSIS_MODEL=llava \
  bun run benchmark:provider-manifest \
  --provider-manifest ./corpus/public-provider-accuracy.json \
  --provider-manifest-cache-dir ./.cache/pdf-corpus
```

`corpus/public-provider-accuracy.json` contains official and publicly available
PDF URLs, pinned SHA256 values, source metadata, full-page visual regions, and
focused public chart, figure, formula, and table crops with expected terms,
expected visual kinds, normalized confidence floors, positive-area bounding
boxes, and capability tags. The crop benchmark writes a
`pdf_provider_manifest_crop_benchmark` artifact with crop provenance and
capability summaries; the provider benchmark uses the same crop path plus
provider normalization and writes a
`pdf_provider_manifest_benchmark` artifact.
The strict release-artifact path runs the crop benchmark against a deterministic
local fixture manifest and provider-manifest scoring against deterministic
local table, formula, chart, figure, and image regions. Public URL downloads
remain opt-in and outside default CI network activity.

## Optimization Tips

### 1. Start With Smart `read_pdf`

Use `read_pdf` first when an agent does not know the document shape. With only
`sources`, it samples a bounded number of pages, counts selectable text and
image paint operations, chooses useful extraction options, and returns the
selected arguments with the Agent Document Twin.

```json
{
  "sources": [{ "path": "doc.pdf" }]
}
```

### 2. Request Only What You Need

```json
// Fast - metadata only
{
  "sources": [{ "path": "doc.pdf" }],
  "include_metadata": true,
  "include_page_count": true,
  "include_full_text": false,
  "include_images": false
}
```

### 3. Search Before Reading Whole Sections

Use `search_pdf` when an agent needs to find relevant evidence before running
larger extraction, rendering, or crop workflows. Search is bounded by page and
match caps.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-50" }],
  "query": "risk controls",
  "max_pages": 50,
  "max_matches_per_source": 10
}
```

### 4. Use Page Ranges

Instead of full text extraction, request specific pages:

```json
{
  "sources": [{
    "path": "doc.pdf",
    "pages": [1, 2]  // Only first two pages
  }],
  "include_full_text": false,
  "include_metadata": false,
  "include_page_count": false,
  "include_images": false
}
```

### 5. Batch Sources

Process multiple PDFs in one request for better throughput:

```json
{
  "sources": [
    { "path": "doc1.pdf" },
    { "path": "doc2.pdf" },
    { "path": "doc3.pdf" }
  ],
  "include_full_text": true,
  "include_metadata": false,
  "include_page_count": false,
  "include_images": false
}
```

### 6. Avoid Images Unless Needed

Image extraction involves encoding to PNG and base64, which adds overhead:

```json
// Slower
{ "include_images": true }

// Faster
{ "include_images": false }
```

### 7. Use The Document Map For Full Agent Navigation

`include_document_map` builds the richest structured response path. It
links pages, elements, selectable text-layer and metadata coverage, chunks,
layout diagnostics, safety findings, trust report routing and signal indexes,
accessibility report routing and issue indexes, visual evidence routing, and page geometry without
embedding image bytes in JSON. It does more work than metadata-only extraction,
but it prevents agents from rebuilding the same references themselves.

Add `include_visual_enrichments` only when the configured visual-region
provider routing plan is needed. It selects bounded table/image regions plus
caption-derived visual regions for vector-drawn formulas, charts, figures, and
diagrams. If a provider is configured, those regions are cropped and analyzed
before normalized evidence is fused back into the document twin. If no provider
is configured, the response still includes the candidate regions so agents can
call `pdf_evidence` operation `extract_regions` or retry analysis later. Keep
`max_visual_enrichments`
small for interactive workflows.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-5" }],
  "include_document_map": true,
  "include_visual_enrichments": true,
  "max_visual_enrichments": 8,
  "include_full_text": false
}
```

### 8. Render Pages With Explicit Bounds

`pdf_evidence` operation `render_page` returns PNG page evidence as MCP image
parts. Rendering is more expensive than text extraction, so select pages, keep
scale practical, and rely on the default pixel budget unless a workflow truly
needs higher resolution.

```json
{
  "operation": "render_page",
  "sources": [{ "path": "doc.pdf", "pages": "1-2" }],
  "scale": 2,
  "max_pages": 2,
  "max_pixels_per_page": 16000000
}
```

### 9. Crop Regions Instead Of Carrying Whole Pages

`pdf_evidence` operation `extract_regions` reuses bounded page rendering but
returns focused crops for specific PDF-coordinate bounding boxes. It is usually
cheaper for downstream vision/OCR steps than passing a whole rendered page.

```json
{
  "operation": "extract_regions",
  "sources": [{
    "path": "doc.pdf",
    "regions": [{
      "id": "table-1",
      "page": 1,
      "bounding_box": { "left": 72, "bottom": 420, "right": 540, "top": 620 }
    }]
  }],
  "scale": 2,
  "max_regions": 20
}
```

### 10. OCR Only The Pages That Need It

`pdf_evidence` operation `ocr_pages` renders selected pages and sends temporary
PNGs to the configured local OCR provider. OCR cost depends on render scale,
page count, provider runtime, and output size, so keep page selections tight.

```json
{
  "operation": "ocr_pages",
  "sources": [{ "path": "scan.pdf", "pages": "1-3" }],
  "scale": 2,
  "max_pages": 3,
  "timeout_ms": 60000,
  "max_output_chars": 200000,
  "languages": ["eng"]
}
```

### 11. Use Structured Elements When You Need References

`include_elements` adds page-level element metadata for agent workflows. It is
worth enabling when you need stable IDs, provenance, or best-effort coordinates,
but plain text remains the leanest response shape.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-5" }],
  "include_elements": true,
  "include_full_text": false
}
```

### 12. Add Semantic Hints Only When They Help

`include_semantic_hints` adds deterministic heading, list, paragraph, caption,
header, and footer hints to text elements, including common numbered sections,
appendix/chapter-style headings, checkbox/bullet list prefixes, and
equation/formula or graph/chart caption aliases. It returns elements even when
`include_elements` is omitted.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-5" }],
  "include_semantic_hints": true,
  "include_full_text": false
}
```

### 13. Use Markdown When You Need Ready-to-Use Context

`include_markdown` creates page-aware Markdown in the JSON response. It is
more convenient than rebuilding sections from `page_texts`, but it still
requires page extraction.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-5" }],
  "include_markdown": true,
  "include_full_text": false
}
```

### 14. Use Chunks When You Need Source References

`include_chunks` creates citation-ready chunks with element IDs, strategy
labels, and best-effort bounding boxes. It can split on semantic heading
boundaries when `include_semantic_hints` is enabled, and it can emit table
chunks when `include_tables` is enabled. It is useful for retrieval and
citations, but it does more work than metadata-only or page-count requests.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-5" }],
  "include_chunks": true,
  "include_semantic_hints": true,
  "include_full_text": false
}
```

### 15. Use HTML Only When Needed

`include_html` creates escaped page-aware HTML. It is useful for preview and
export workflows, but plain text or Markdown are usually leaner for agent-only
context.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-5" }],
  "include_html": true,
  "include_full_text": false
}
```

### 16. Use Layout Diagnostics For Routing

`include_layout_diagnostics` returns page layout profiles, reading-order
confidence, column signals, and warnings. It uses already extracted content
geometry and does not add OCR, vision, or parser dependencies. It is useful
before unattended RAG indexing or citation-critical summarization.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-5" }],
  "include_layout_diagnostics": true,
  "include_chunks": true,
  "include_full_text": false
}
```

### 17. Use Document Signals For Bounded Structure

Outline, page labels, permissions, structure trees, form fields, attachment
metadata, and page geometry can be requested without extracting full page text.
Annotations, structure trees, and page geometry respect selected page ranges.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-5" }],
  "include_outline": true,
  "include_structure_tree": true,
  "include_page_geometry": true,
  "include_full_text": false
}
```

### 18. Use Accessibility Reports Instead Of Raw Structure Dumps

`include_accessibility_report` summarizes tagged-PDF coverage, structure tree
availability, tag-to-visible-content coverage, headings, images, links, forms,
accessibility permissions, issue types, severities, page grades, and
affected-page counts in one compact report. Prefer it when an agent needs
routing guidance instead of the full raw structure tree or annotation payload.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-5" }],
  "include_accessibility_report": true,
  "include_full_text": false
}
```

### 19. Use Text Layers For Run, Line, Word, And Character Evidence

`include_text_layer` keeps run, line, word, and character references in
structured JSON with page-level ranges, estimated bounding boxes,
direction-aware right-to-left ordering, and run-metadata coverage counts.
Prefer it when an agent needs text evidence anchors but does not need full raw
page content.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-5" }],
  "include_text_layer": true,
  "include_full_text": false
}
```

### 20. Use Safety Findings When Agents Consume PDF Text

`include_safety_findings` scans extracted page text for deterministic risk
signals, including prompt-injection-like text, tiny text, off-page text, and
hidden or near-invisible text geometry, and overlapping text that may visually
spoof or obscure content. `include_trust_report` can consolidate those text
signals with annotation-derived unsafe link schemes, `trust_report_redaction`
can select standard, strict, or explicit off evidence-snippet handling, and
`include_document_map` can link the trust report back to page-level risk
routing.
Safety findings require page text extraction, but they do not force `full_text`
into the JSON response.

```json
{
  "sources": [{ "path": "doc.pdf", "pages": "1-5" }],
  "include_safety_findings": true,
  "include_full_text": false
}
```

## Concurrency

The server processes multiple sources concurrently with a default limit of 3 simultaneous operations to prevent memory exhaustion.

## File Size Limits

- Maximum file size: 100MB
- Files exceeding this limit will return an error

## Memory Usage

Memory usage scales with:
- Number of concurrent sources
- PDF complexity
- Image extraction enabled

For large PDFs or many concurrent requests, ensure adequate system memory.
