# PDF Search Evidence

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

Date: 2026-06-15
Status: active

## Goal

Let agents find relevant PDF evidence before reading entire page ranges,
rendering pages, cropping regions, or constructing citations.

`search_pdf` performs bounded literal search over extracted PDF text and,
when explicitly requested, a configured OCR text layer. It returns page-level
evidence: matched text, snippets, offsets, text-item or OCR word indexes,
optional character-derived, text-item, or OCR-word bounding boxes, and
provenance.

## Non-Goals

- Do not accept arbitrary regular expressions in request payloads.
- Do not build a persistent index or cache in the first slice.
- Do not run OCR unless `include_ocr_text_layer` is explicitly enabled.
- Do not claim semantic search or embedding retrieval.

## Public Contract

```json
{
  "sources": [{ "path": "report.pdf", "pages": "1-20" }],
  "query": "risk controls",
  "whole_word": true,
  "include_ocr_text_layer": false,
  "max_matches_per_source": 10
}
```

The first content part is JSON:

```json
{
  "profile": "pdf_search_results",
  "search_options": {
    "query": "risk controls",
    "case_sensitive": false,
    "whole_word": true,
    "include_ocr_text_layer": false,
    "max_pages": 100,
    "max_matches_per_source": 10,
    "context_chars": 120
  },
  "results": [{
    "source": "report.pdf",
    "success": true,
    "num_pages": 42,
    "searched_pages": [1, 2, 3],
    "total_matches": 1,
    "matches": [{
      "id": "p2-match-1",
      "page": 2,
      "text": "risk controls",
      "snippet": "The risk controls were reviewed...",
      "match_start": 4,
      "match_end": 17,
      "text_item_index": 3,
      "bounding_box": { "left": 72, "bottom": 620, "right": 280, "top": 632 },
      "bounding_box_level": "char_estimated",
      "provenance": {
        "engine": "pdfjs",
        "source": "text-content"
      }
    }]
  }]
}
```

## Invariants

- Search is literal only; no regex execution from request payloads.
- If no source page range is provided, search starts from page 1 and is bounded
  by `max_pages`.
- `max_pages` defaults to 100 and is capped at 1000 per source.
- `max_matches_per_source` defaults to 50 and is capped at 500.
- `context_chars` defaults to 120 and is capped at 1000.
- OCR-layer search is disabled by default. When enabled, pages are rendered and
  passed through the configured local OCR provider; OCR failures become source
  warnings rather than silently mixing OCR with selectable text.
- Whole-word matching uses ASCII word boundaries.
- Bounding boxes prefer estimated character evidence when the extracted text
  layer has run/character boxes, then fall back to best-effort text-item boxes.
  OCR matches use provider word boxes when available.
- Character-derived boxes are still estimated geometry, not glyph-perfect
  geometry.
- Each source result preserves success/failure isolation.
- Search output contains no image bytes.

## Follow-On Work

- OCR search fixture benchmarks against scanned PDFs.
- Optional `get_evidence` tool for resolving match IDs or element IDs into
  visual crops without repeating bbox arguments manually.
- Persistent local indexes only if repeated-agent workloads justify the
  operational overhead.

## Acceptance Criteria

- `search_pdf` validates source, query, page cap, match cap, context, and match
  mode inputs.
- Unit tests cover page resolution, literal matching, whole-word matching,
  case-sensitive matching, snippets, match IDs, char-derived boxes, OCR word
  boxes, and bbox provenance.
- MCP stdio and HTTP tool lists expose `search_pdf`.
- Integration tests call `search_pdf` through the built server.
- Full validation passes before merge.
