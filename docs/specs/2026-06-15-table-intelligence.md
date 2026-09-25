# Table Intelligence

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

Date: 2026-06-15
Status: shipped

> V3 note: visual table verification now uses `pdf_evidence` operation
> `extract_regions` or `render_page` for source evidence.

## Goal

Make deterministic table extraction more useful for agents without introducing
a heavy visual table recognition dependency. The table contract should tell an
agent not only what rows were extracted, but also how trustworthy the grid is,
which cells were inferred, and whether adjacent pages look like the same table.

## Contract

When `include_tables` is enabled, each extracted table may include:

- `cells[].rowSpan` and `cells[].colSpan` with conservative inferred span
  hints.
- `cells[].isHeader` for the first detected row.
- `cells[].inferred` when the grid cell exists because the table model inferred
  an empty slot.
- `quality.completeness`, `nonEmptyCellRatio`, `cellBoundingBoxCoverage`,
  `inferredCellRatio`, `rowAlignment`, and `rowSpacingConsistency`.
- `quality.cellBoundingBoxCount`, `quality.inferredCellCount`,
  `quality.missingCellCount`, and `quality.mergedCellCandidateCount`.
- `quality.signals` and `quality.warnings` for sparse, merged, irregular,
  incomplete-geometry, low-confidence, or continuation-candidate tables.
- `provenance.source`, which is `selectable_text` for PDF text-coordinate
  extraction or `ocr_text_layer` for OCR word-box extraction.
- `continuation` candidates linking repeated-header or page-edge
  geometry-compatible tables on adjacent pages.

The existing `rows`, Markdown rendering, table chunks, and document map element
IDs remain backward compatible.

## Boundaries

This is a deterministic coordinate model for selectable PDF text and OCR word
boxes. It does not claim to perform full visual table structure recognition,
complete logical table merging for every multi-page layout, or ML-grade
row/column span reconstruction. Those belong behind an optional visual table
provider that can enrich the same table contract later.

## Agent Workflow

Agents should use table quality signals as routing metadata:

- High completeness and complete-grid signals can be used directly for routine
  RAG and summarization.
- Sparse, merged, incomplete-geometry, irregular, or low-confidence warnings
  should trigger `extract_regions` or `render_page` for source verification.
- Continuation candidates should be treated as linked evidence, not as a
  merged logical table unless the caller confirms that behavior.

## Acceptance Criteria

- Table cells expose header/span/inference hints without changing existing
  row arrays.
- OCR word boxes can generate OCR-derived tables for scanned pages when
  `include_ocr_text_layer` and `include_tables` are both enabled.
- OCR-derived tables are deduplicated by bounding-box overlap, preserving
  distinct scanned tables on mixed selectable-text/OCR pages.
- Sparse tables expose missing-cell and inferred-cell warnings.
- Tables expose cell bounding-box coverage and inferred-cell ratios for
  evidence-quality routing.
- Wide text boxes crossing column boundaries expose merged-cell candidate
  signals.
- Repeated headers on adjacent pages expose continuation candidates.
- Page-edge tables with matching column geometry can expose continuation
  candidates even when the next page does not repeat the header row.
- `read_pdf` table summaries and the agent document map preserve the new
  quality metadata.
