---
layout: home

hero:
  name: Citra
  text: Give your AI agent eyes for PDFs — with proof.
  tagline: "Zero-config local PDF evidence for agents. `npx -y @sylphx/citra` → structured text, tables, OCR, page-level citations. Plain-text tools make agents guess. Citra returns proof."
  image:
    src: /logo.svg
    alt: Citra Logo
  actions:
    - theme: brand
      text: Zero-config npx
      link: /guide/installation
    - theme: alt
      text: Star on GitHub
      link: https://github.com/SylphxAI/citra
    - theme: alt
      text: Stop PDF Hallucinations
      link: /articles/stop-pdf-hallucinations
    - theme: alt
      text: Product Proof
      link: /guide/product-proof
    - theme: alt
      text: Benchmark Proof
      link: /benchmark

features:
  - icon: "🚀"
    title: Zero-config
    details: "`npx -y @sylphx/citra` starts a brand-sole MCP server on stdio. No Docker, no API key, no global install."

  - icon: "\U0001F441\uFE0F"
    title: Evidence, not text dumps
    details: Page numbers, bounding boxes, table cells, and provenance so agents can cite instead of invent.
  - icon: "\u26A1"
    title: Measured product proof
    details: Cleaner install than historical TS 3.0.14 on measured linux-x64, and registry-bound dual-mode warm re-reads ≥ ~10× on required fixtures (method-bounded).
  - icon: "\U0001F4CA"
    title: Tables agents can trust
    details: Rows, cells, geometry, and confidence for financial reports and structured PDFs.
  - icon: "\U0001F50E"
    title: Search then verify
    details: Find snippets with page context before deep reading, cropping, or citing.
  - icon: "\U0001F4C4"
    title: Agent Document Twin
    details: One linked document map — structure, chunks, layout, trust and accessibility routing when requested.
  - icon: "\U0001F5BC\uFE0F"
    title: Visual evidence
    details: Render pages and crop regions for tables, figures, charts, and citations.
  - icon: "\U0001F524"
    title: Scanned PDF OCR path
    details: Route selected pages through configured OCR providers and keep OCR separate from selectable text.
  - icon: "\U0001F6E1\uFE0F"
    title: Trust signals
    details: Surface hidden-text, prompt-injection, overlapping, and related risk signals when requested.
  - icon: "\u26A1"
    title: Native local engine
    details: Sole-Rust production on five platforms via a thin Node launcher. Clean install. Fail closed if the native binary is missing.
  - icon: "\U0001F50C"
    title: Three tools, one surface
    details: read_pdf for smart extraction, search_pdf for cheap evidence, pdf_evidence for focused inspect/render/crop/OCR ops.
---
