---
layout: home
title: "anymd: any file to clean Markdown for AI agents, local MCP server and CLI"
titleTemplate: false

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

features:
  - icon:
      light: /icons/zap.svg
      dark: /icons/dark/zap.svg
    title: Fast
    details: "Native Rust converts in parallel, page by page, with no models to load. The benchmarks list the measured times."
    link: /guide/benchmarks
  - icon:
      light: /icons/table-2.svg
      dark: /icons/dark/table-2.svg
    title: Accurate layout
    details: Words rebuilt from glyph gaps, two-column papers in reading order, tables recovered, including borderless ones. No glued words, no scrambled columns.
    link: /guide/formats#pdf
  - icon:
      light: /icons/scan-text.svg
      dark: /icons/dark/scan-text.svg
    title: Token-lean
    details: Markdown with page anchors, a small front-matter header, and compact tables. A token budget and a cursor keep big documents inside your context.
    link: /guide/tools#read
  - icon:
      light: /icons/file-text.svg
      dark: /icons/dark/file-text.svg
    title: Every format
    details: One read call handles PDF, Word, PowerPoint, Excel, CSV, EPUB, HTML and URLs, images, audio, and video.
    link: /guide/formats
  - icon:
      light: /icons/search.svg
      dark: /icons/dark/search.svg
    title: Search everything
    details: Search files, whole folders, and URLs at once. Exact phrases first, BM25-ranked passages when nothing matches exactly.
    link: /guide/tools#search
  - icon:
      light: /icons/hard-drive.svg
      dark: /icons/dark/hard-drive.svg
    title: Local & private
    details: Nothing is uploaded and no API key is needed. OCR and transcripts run on your machine, and a model downloads only when you ask for it.
    link: /guide/security
---

<div class="cit-section">

<!-- generated:formerly -->
<p class="cit-fine" style="text-align:center">Formerly <strong>pdf-reader-mcp</strong>. See <a href="./guide/migration">Migration</a>.</p>
<!-- /generated:formerly -->

<video src="/demo.mp4" poster="/demo-poster.png" autoplay muted loop playsinline preload="metadata" width="820" style="display:block;margin:32px auto;max-width:100%;height:auto;border-radius:12px" aria-label="Real terminal session: anymd converts a PDF, searches a folder, reads a spreadsheet, and Claude Code answers through the anymd MCP server"></video>

## Quick start

```bash
claude mcp add anymd -- npx -y @sylphx/anymd   # add it to your agent
npx -y @sylphx/anymd report.pdf > report.md     # or convert from the shell
npx -y @sylphx/anymd search "indemnification" contracts/
```

Other clients (Codex, Cursor, VS Code, Claude Desktop, and more) are in [Getting started](/guide/getting-started).

## Need proof, not just text?

[anymd Pro](/pro) lets your agent cite exact video frames and verify that a quote is on the page it claims. US$29 once, works offline. The core stays free and MIT licensed.

## Also from Sylphx

<!-- generated:also-from -->
- [**repomap**](https://github.com/SylphxAI/repomap): A map of your codebase for AI agents: code graph, search, call paths and change impact.
- [**lockdocs**](https://github.com/SylphxAI/lockdocs): Exact-version library docs from your lockfile. Local, offline, no rate limits.
- [**skills**](https://github.com/SylphxAI/skills): Battle-tested agent skills for Claude Code and Codex, installed in one command.
- [**readme-mark**](https://github.com/SylphxAI/readme-mark): Beautiful README images from one URL: banners, badges, icons and stats cards.
<!-- /generated:also-from -->

More from Sylphx: https://sylphx.com/open-source

</div>
