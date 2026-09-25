---
layout: home

hero:
  name: anymd
  text: Any file → clean Markdown for AI agents
  tagline: PDF, Office, EPUB, HTML, images, audio/video. Fast Rust MCP server + CLI. Local, no API key.
  image:
    src: /logo.svg
    alt: anymd
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: Benchmarks
      link: /guide/benchmarks
    - theme: alt
      text: GitHub
      link: https://github.com/SylphxAI/anymd

features:
  - icon:
      src: /icons/zap.svg
    title: Fast
    details: Native Rust converts in parallel, page by page. The 15-page Attention Is All You Need paper takes 0.08 s; MarkItDown needs 2.2 s.
    link: /guide/benchmarks
  - icon:
      src: /icons/table-2.svg
    title: Accurate layout
    details: Words rebuilt from glyph gaps, two-column papers in reading order, tables recovered, including borderless ones. No glued words, no scrambled columns.
    link: /guide/formats#pdf
  - icon:
      src: /icons/scan-text.svg
    title: Token-lean
    details: Markdown with page anchors, a small front-matter header, and compact tables. A token budget and a cursor keep big documents inside your context.
    link: /guide/tools#read
  - icon:
      src: /icons/file-text.svg
    title: Every format
    details: One read call handles PDF, Word, PowerPoint, Excel, CSV, EPUB, HTML and URLs, images, audio, and video.
    link: /guide/formats
  - icon:
      src: /icons/search.svg
    title: Search everything
    details: Search files, whole folders, and URLs at once. Exact phrases first, BM25-ranked passages when nothing matches exactly.
    link: /guide/tools#search
  - icon:
      src: /icons/hard-drive.svg
    title: Local & private
    details: Nothing is uploaded and no API key is needed. OCR and transcripts use local tools you already have, only when they are installed.
    link: /guide/security
---

<div class="cit-section">

<p class="cit-fine" style="text-align:center">Formerly <strong>pdf-reader-mcp</strong> / <strong>Citra</strong>. <code>@sylphx/pdf-reader-mcp</code> and <code>@sylphx/citra</code> still install and run anymd. See <a href="./guide/migration">Migration</a>.</p>

<img src="/demo.svg" alt="anymd converting a PDF, searching a folder, and reading a spreadsheet" style="display:block;margin:32px auto;max-width:100%" />

## Quick start

```bash
claude mcp add anymd -- npx -y @sylphx/anymd   # add it to your agent
npx -y @sylphx/anymd report.pdf > report.md     # or convert from the shell
npx -y @sylphx/anymd search "indemnification" contracts/
```

Other clients (Codex, Cursor, VS Code, Claude Desktop, and more) are in [Getting started](/guide/getting-started).

</div>
