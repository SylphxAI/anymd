# Weekly Update: V3 PDF Intelligence

PDF Reader MCP V3 is a major step toward agent-grade PDF understanding with a
much cleaner MCP surface.

The headline change is simple: agents can now start with one smart call.
`read_pdf` can inspect the document, choose a high-value extraction route, and
return an structured document result with Markdown, chunks, document maps, table
signals, layout routing, trust signals, accessibility routing, OCR provenance,
and visual evidence hooks.

The public tool surface is also smaller and easier to learn:

- `read_pdf` for smart structured document result extraction.
- `search_pdf` for cheap source-backed text evidence.
- `pdf_evidence` for focused inspect, render, crop, OCR, and visual-analysis
  operations.

This keeps the agent context lean while preserving the advanced evidence paths
needed for scanned PDFs, tables, charts, formulas, figures, citations, trust
reviews, and accessibility work.

V3 continues to be TypeScript-first, local-first, and benchmark-gated. The
release process verifies the package contract, docs, deterministic quality
checks, corpus coverage, provider evidence, crop evidence, and the SOTA release
gate before publication.
