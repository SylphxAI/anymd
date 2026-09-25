---
layout: home

hero:
  name: anymd
  # generated:hero
  text: "Any file → clean Markdown for AI agents"
  tagline: "PDF, Word, PowerPoint, Excel, EPUB, HTML and web pages, images (OCR), audio and video (metadata, subtitles, transcripts). A fast Rust MCP server and CLI that runs on your machine. No API key."
  # /generated:hero
  image:
    src: /logo.svg
    alt: anymd
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: Try it in your browser
      link: /playground
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
    # generated:bench-fast
    details: "Native Rust converts in parallel, page by page. The 15-page Attention Is All You Need paper takes 0.13 s; MarkItDown needs 3.0 s."
    # /generated:bench-fast
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

<!-- generated:formerly -->
<p class="cit-fine" style="text-align:center">Formerly <strong>pdf-reader-mcp</strong>. See <a href="./guide/migration">Migration</a>.</p>
<!-- /generated:formerly -->

<img src="/demo.gif" alt="Real terminal session: anymd converts a PDF, searches a folder, reads a spreadsheet, and Claude Code answers through the anymd MCP server" style="display:block;margin:32px auto;max-width:100%" />

## Quick start

```bash
claude mcp add anymd -- npx -y @sylphx/anymd   # add it to your agent
npx -y @sylphx/anymd report.pdf > report.md     # or convert from the shell
npx -y @sylphx/anymd search "indemnification" contracts/
```

Other clients (Codex, Cursor, VS Code, Claude Desktop, and more) are in [Getting started](/guide/getting-started).

## Also from Sylphx

<!-- generated:also-from -->
- [**repomap**](https://github.com/SylphxAI/repomap): A map of your codebase for AI agents: code graph, search, call paths and change impact — with an interactive graph UI. Rust MCP server + CLI. Local, no API key, MIT.
- [**lockdocs**](https://github.com/SylphxAI/lockdocs): Exact-version library docs from your lockfile — local, offline, no rate limits.
- [**readme-mark**](https://github.com/SylphxAI/readme-mark): Beautiful README images from one URL — animated banners, shields-compatible badges, typing text, 3000+ tech icons, GitHub stats cards. Free, no token, drop-in for shields / capsule-render / skill-icons / readme-typing-svg / github-readme-stats.
<!-- /generated:also-from -->

</div>
