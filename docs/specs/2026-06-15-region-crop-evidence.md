# Region Crop Evidence

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

> V3 note: region cropping is now exposed through `pdf_evidence` operation
> `extract_regions`.

Date: 2026-06-15
Status: active

## Goal

Let agents request focused visual evidence for table, figure, chart, formula,
annotation, and citation bounding boxes without carrying whole-page images or
duplicating image bytes inside JSON summaries.

## Public Contract

`extract_regions` accepts PDF sources with one or more regions:

```json
{
  "sources": [{
    "path": "report.pdf",
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

The first content part is JSON:

```json
{
  "profile": "region_crop_evidence",
  "crop_options": {
    "scale": 2,
    "max_regions": 20,
    "max_pixels_per_page": 16000000,
    "include_image": true
  },
  "results": [{
    "source": "report.pdf",
    "success": true,
    "regions": [{
      "region_id": "table-1",
      "page": 1,
      "evidence_id": "page-1-table-1-crop-scale-2",
      "source_bounding_box": { "left": 72, "bottom": 420, "right": 540, "top": 620 },
      "crop_pixels": { "left": 144, "top": 344, "width": 936, "height": 400 },
      "image_content_index": 1,
      "provenance": {
        "engine": "pdfjs",
        "renderer": "@napi-rs/canvas",
        "source": "region-crop",
        "page_render_evidence_id": "page-1-render-scale-2"
      }
    }]
  }]
}
```

Cropped PNG data is returned as MCP image content parts after the JSON part.
The JSON summary must not include cropped region base64.

## Invariants

- Region bounding boxes use PDF coordinates: `left`, `bottom`, `right`, `top`.
- Invalid boxes where `right <= left` or `top <= bottom` are rejected.
- `max_regions` defaults to 20 and is capped by schema at 100 per source.
- `max_pixels_per_page` applies before cropping, because the full page must be
  rendered once per requested page.
- Each source result preserves success/failure isolation.
- Crop output links back to the page render evidence ID used to create it.

## Follow-On Work

- Rotation-aware viewport mapping for PDFs with rotated pages.
- Direct integration from document-map element IDs to region crop requests.
- Engine-specific chart, formula, and visual table provider presets that
  consume crops through `analyze_regions`.
- Optional crop-level OCR routing after page-level provider benchmarks are
  stable.
