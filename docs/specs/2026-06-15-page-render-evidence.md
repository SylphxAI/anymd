# Page Render Evidence

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

> V3 note: page rendering is now exposed through `pdf_evidence` operation
> `render_page`.

Date: 2026-06-15
Status: active

## Goal

Give agents a bounded way to inspect the original visual page, route sparse or
scanned pages toward OCR, and verify layout claims without duplicating image
bytes inside JSON summaries.

## Public Contract

`render_page` accepts the same source shape as `read_pdf`:

```json
{
  "sources": [{ "path": "report.pdf", "pages": "1-2" }],
  "scale": 2,
  "max_pages": 2
}
```

The first content part is JSON:

```json
{
  "profile": "page_render_evidence",
  "render_options": {
    "scale": 2,
    "max_pages": 2,
    "max_pixels_per_page": 16000000,
    "include_image": true
  },
  "results": [
    {
      "source": "report.pdf",
      "success": true,
      "num_pages": 10,
      "rendered_pages": [
        {
          "page": 1,
          "evidence_id": "page-1-render-scale-2",
          "width": 1224,
          "height": 1584,
          "scale": 2,
          "pixel_count": 1938816,
          "byte_length": 240000,
          "format": "png",
          "mime_type": "image/png",
          "image_content_index": 1,
          "provenance": {
            "engine": "pdfjs",
            "renderer": "@napi-rs/canvas",
            "source": "page-render"
          }
        }
      ]
    }
  ]
}
```

Rendered PNG data is returned as MCP image content parts after the JSON part.
The JSON summary must not include rendered page base64.

## Invariants

- If no page range is supplied, render only page 1.
- `max_pages` defaults to 5 and is capped by schema at 20 per source.
- `max_pixels_per_page` defaults to 16MP and is capped by schema at 64MP.
- Each source result preserves success/failure isolation.
- Unknown renderer failures are logged internally and surfaced as curated
  messages.
- Visual evidence uses the same source vocabulary as `read_pdf`.

## Follow-On Work

- OCR provider hardening with scanned fixtures and provider presets.
- Advanced viewport transforms for rotated-page crop evidence.
- Engine-specific chart, formula, and visual table provider presets and
  accuracy fixtures.
- Benchmark fixtures for scanned, sparse, mixed-layout, and visually dense
  pages.
