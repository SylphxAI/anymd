# Predictable defaults

A `read_pdf` call with only `sources` uses **fast**.

Fast returns markdown, tables, chunks, a document map, page geometry, layout,
and semantic hints, plus metadata and the page count. It does not return a
trust report, safety findings, or an accessibility report, and it does not run
OCR or render pages.

| Profile | Adds | Never includes |
| --- | --- | --- |
| `fast` (default) | the lean read above | audits, OCR, rendering |
| `quality` | text layer, elements, HTML, document AST, outline, annotations, forms, attachments, structure tree, permissions, full text, page labels | audits, OCR, rendering |
| `research` | everything in quality, plus safety, trust, and accessibility | OCR, rendering |
| `balanced` | fast plus safety, trust, and accessibility. Selected by `auto: true`, not by omitting options | structure extras, OCR, rendering |

`auto_detail` wins over `profile`. `pages` is a filter: a page list still uses
the preset and reads every page you named. `sample_pages` belongs to
`pdf_evidence` inspect, not to `read_pdf`.

Pass `auto: false` or any `include_*` flag to take manual control. OCR is
`include_ocr_text_layer` or `pdf_evidence`.
