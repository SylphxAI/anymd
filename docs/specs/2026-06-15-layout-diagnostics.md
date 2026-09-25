# Layout Diagnostics

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

Date: 2026-06-15
Status: active

## Goal

Expose lightweight, deterministic layout confidence signals so agents can
decide whether local PDF extraction is safe for indexing, summarization, and
citation-critical workflows.

The feature should improve routing quality without adding OCR, vision models,
Java, Python, or external parser dependencies.

The extraction order now uses conservative recursive layout segmentation:
large horizontal whitespace splits page content into visual bands, then each
band can be column-segmented independently. This handles common spanning
headers, independent multi-column sections, and footers better than a single
page-level column cut.

## Contract

`read_pdf` accepts:

```json
{
  "sources": [{ "path": "report.pdf", "pages": "1-5" }],
  "include_layout_diagnostics": true
}
```

When enabled, each successful source may include `layout_diagnostics`:

```json
{
  "layout_diagnostics": [
    {
      "page": 1,
      "profile": "multi_column",
      "reading_order": "columnar",
      "confidence": 0.86,
      "item_count": 12,
      "text_item_count": 12,
      "image_item_count": 0,
      "positioned_item_ratio": 1,
      "column_count": 2,
      "columns": [
        { "index": 1, "left": 40, "right": 210, "item_count": 6 },
        { "index": 2, "left": 330, "right": 500, "item_count": 6 }
      ],
      "signals": ["text-items", "positioned-items", "two-column-layout"]
    }
  ]
}
```

Profiles:

- `single_column`
- `multi_column`
- `mixed_layout`
- `image_or_sparse`
- `unknown`

Reading-order models:

- `natural`
- `columnar`
- `mixed`
- `uncertain`

## Invariants

- Disabled by default for backward compatibility.
- Uses existing page content geometry; no second parser pass.
- Reading order is deterministic recursive XY-cut style segmentation over
  extracted bounding boxes, not a visual model.
- Does not decode image bytes unless `include_images` is also requested.
- Does not claim OCR, vision, or accessibility remediation.
- Emits warnings when confidence is low, coordinates are sparse, or positioned
  items overlap.

## Validation

- Unit/eval coverage verifies recursive band ordering and layout confidence
  signals.
- Handler coverage verifies the public `read_pdf` flag and JSON shape.
- Full validation includes typecheck, Biome, build, tests, and docs build.
