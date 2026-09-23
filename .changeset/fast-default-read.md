---
"@sylphx/citra": major
---

`read_pdf` defaults to the fast preset.

A call with only `sources` — including a source materialized as `"pages": null` — returns markdown, tables, chunks, a document map, page geometry, layout, and semantic hints, plus metadata and the page count. It no longer returns safety findings, a trust report, or an accessibility report, and it does not sample pages.

`profile: "quality"` adds structure (text layer, HTML, elements, document AST, outline, annotations, forms, attachments, structure tree, permissions, full text, page labels) and still does not run OCR. `profile: "research"` adds safety, trust, and accessibility on top of quality. `auto: true` keeps the previous balanced preset. `auto_detail` wins over `profile`. `auto: false` or any `include_*` flag stays manual. A page list filters the read and does not turn the preset off. OCR and rendering stay on `include_ocr_text_layer` or `pdf_evidence`.
