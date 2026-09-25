---
title: Playground
description: Try anymd in your browser. Drop a PDF, Word, PowerPoint, Excel, EPUB, or HTML file and get clean Markdown, converted on your device with WebAssembly.
aside: false
---

# Playground

Drop a file and get the Markdown an agent would read. This page runs the same
Rust engine as the anymd CLI and MCP server, compiled to WebAssembly, inside a
Web Worker on your device.

::: tip Your files never leave your browser
The file is read with the browser's File API and converted locally. Nothing is
uploaded, and there is no server behind this page. You can go offline once it
has loaded and keep converting.
:::

<ClientOnly>
  <Playground />
</ClientOnly>

## What the browser build covers

| Format | In the browser | With the CLI |
|-|-|-|
| PDF | Full layout engine: columns, headings, tables, page markers | Also OCR of scanned pages (local `tesseract`) |
| DOCX, PPTX, XLSX/ODS, CSV/TSV, EPUB, HTML, text, SRT/VTT | Same output as the CLI | Same |
| Images | Format, dimensions, EXIF | Also OCR (`--ocr`) |
| Audio / video | Container type and size | Duration, streams, chapters, subtitles (ffmpeg), transcript (whisper.cpp) |

The output uses the CLI conventions: a small front-matter header (`source`,
`format`, `title`, `pages`/`slides`/`sheets`) and `<!-- page N -->` style
markers an agent can cite. The token count is the same estimate the CLI uses
for its budget.

For folders, URLs, token budgets, and agent integration, install the CLI or the
MCP server: see [Getting started](/guide/getting-started).
