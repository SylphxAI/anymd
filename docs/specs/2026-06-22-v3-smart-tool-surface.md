# V3 Smart Tool Surface

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

## Goal

PDF Reader MCP V3 makes the agent workflow simpler without reducing document
intelligence. The public MCP surface is intentionally compact:

- `read_pdf` is the primary entrypoint and can automatically inspect, route, and
  read each PDF into an Agent Document Twin.
- `search_pdf` stays separate because literal evidence search is cheap, common,
  and useful before broad extraction.
- `pdf_evidence` consolidates focused evidence operations: `inspect`,
  `render_page`, `extract_regions`, `ocr_pages`, and `analyze_regions`.

## Non-Goals

- Do not rewrite the PDF engine. Existing render, crop, OCR, visual-provider,
  table, trust, accessibility, and benchmark internals remain reusable.
- Do not bundle OCR, vision, formula, chart, or layout model weights.
- Do not claim external certification, PDF/UA compliance, or model accuracy that
  is not backed by release artifacts.

## Contract

`read_pdf` defaults to automatic routing when no explicit `include_*` option is
supplied. Explicit options still run precise manual extraction. `auto_detail`
controls the automatic output depth:

- `fast`: core document map, chunks, Markdown, tables, semantic hints, layout
  diagnostics, metadata, page count, and page geometry.
- `balanced`: `fast` plus safety findings, trust report, and accessibility
  report.
- `full`: `balanced` plus full text, HTML, elements, text layer, document AST,
  outline, annotations, page labels, permissions, forms, attachments, and
  structure tree where available.

`pdf_evidence` takes one required `operation` field so agents learn one evidence
tool instead of separate render, crop, OCR, inspect, and visual-analysis tools.

## Acceptance Criteria

- MCP stdio and HTTP tool lists expose `read_pdf`, `search_pdf`, and
  `pdf_evidence`.
- `read_pdf` with only `sources` returns `auto_read` routing metadata and a
  linked Agent Document Twin response.
- `pdf_evidence` operation tests cover inspect, render, crop, OCR, and visual
  analysis through the public MCP server.
- README, API docs, guide, design, comparison, and release-facing docs describe
  the V3 surface without listing removed public tool names as first-class tools.
- Release remains gated by typecheck, lint/check, tests, docs build, package
  smoke, benchmark artifacts, and the SOTA release gate.
