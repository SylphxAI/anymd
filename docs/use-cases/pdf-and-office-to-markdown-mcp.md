---
title: "Convert PDF, Word, Excel & Media to Clean Markdown Locally (MCP + CLI) | anymd"
titleTemplate: false
description: "Fast local Rust MCP server and CLI turning PDF, DOCX, PPTX, XLSX, EPUB, HTML, OCR images, and video transcripts into token-lean Markdown. 96.4 on AgentDocBench (11.5 s for 38 docs). Free, MIT, no API key."
image: https://anymd.sylphx.com/og-image.png
---

Free MIT Rust MCP server & CLI · No API key

# Any file to clean Markdown for AI agents—locally in milliseconds.

Python document converters take minutes to load models or scramble multi-column PDFs and tables. `anymd` is a native Rust binary and MCP server that parses PDF, Word, PowerPoint, Excel, EPUB, HTML, OCR images, and audio/video directly on your machine—scoring **96.4** across 38 documents in **11.5 seconds** on [AgentDocBench](/docs/benchmarks?entry_surface=seo_pdf-and-office-to-markdown-mcp).

[Install via npx -y @sylphx/anymd setup](/docs?entry_surface=seo_pdf-and-office-to-markdown-mcp) &nbsp;&nbsp; [See AgentDocBench results](/docs/benchmarks?entry_surface=seo_pdf-and-office-to-markdown-mcp)

## Why teams switch to `anymd`

### 1. One command wires Claude Code, Codex, Cursor, VS Code &amp; Claude Desktop

Run `npx -y @sylphx/anymd setup` (or install via `cargo install anymd`, `brew install sylphxai/tap/anymd`, Scoop from the `SylphxAI/scoop-bucket` bucket, or Docker `ghcr.io/sylphxai/anymd`). Zero Python virtualenvs, zero LibreOffice installs, and zero API keys.

### 2. Reading-order Markdown and intact tables without burning context tokens

`anymd` rebuilds words from PDF glyph gaps, orders two-column papers correctly, recovers borderless tables, strips running headers, and adds `<!-- page N -->` anchors. Use `outline` to jump straight to a section and `read` with a token budget so huge files stay inside context.

### 3. 100% local privacy—plus verifiable PDF quote checks and video frames with Pro

Files stay on your machine (`--allow-dir` locks MCP scope to one folder). Need deterministic PDF quote verification (`cite_check`) or SHA-256 verifiable video frames? [anymd Pro](/pricing?entry_surface=seo_pdf-and-office-to-markdown-mcp) unlocks both offline in the same binary for a one-time **US$29** payment. See [Local core and anymd Pro](/docs/hosted?entry_surface=seo_pdf-and-office-to-markdown-mcp).

## Measured on AgentDocBench (38 Docs / 12 Categories)

| Converter | Overall Score | Docs Converted | Table Cells F1 | Time (All 38 Docs) | Output Tokens |
|---|---|---|---|---|---|
| **anymd (Rust, local MIT)** | **96.4** | **38 / 38** | **92.2** | **11.5 s** | **368,659** |
| Docling | 93.0 | 38 / 38 | 89.9 | 2,432.4 s | 430,148 |
| Kreuzberg | 81.7 | 38 / 38 | 38.4 | 16.0 s | 455,689 |
| Unstructured | 81.2 | 38 / 38 | 38.4 | 346.5 s | 414,333 |
| MarkItDown | 76.8 | 38 / 38 | 57.2 | 75.6 s | 538,388 |
| Marker | 71.0 | 30 / 38 | 60.9 | 7,104.5 s | 277,779 |
| `pdftotext` | 42.2 | 23 / 38 | 0.0 | 0.90 s | 201,448 |

Coming from MarkItDown? See [anymd vs MarkItDown](/vs/markitdown?entry_surface=seo_pdf-and-office-to-markdown-mcp) for a side-by-side comparison and a one-line switch. Comparing Python pipelines? See [anymd vs Docling](/vs/docling?entry_surface=seo_pdf-and-office-to-markdown-mcp), [Marker](/vs/marker?entry_surface=seo_pdf-and-office-to-markdown-mcp) or [Unstructured](/vs/unstructured?entry_surface=seo_pdf-and-office-to-markdown-mcp).

## FAQ

**What file formats does `anymd` convert with zero external cloud calls?**
One `read` call handles PDF, Word (`.docx`), PowerPoint (`.pptx`), Excel/OpenDocument (`.xlsx`, `.xls`, `.ods`, CSV/TSV), EPUB, HTML and URLs, images (EXIF + OCR via local doc-VLM or Tesseract), and audio/video metadata, subtitles, and transcripts.

**Does `anymd` require an API key or upload my documents?**
No. Everything runs locally on your machine under the MIT licence, and no network call is made unless you explicitly pass an `http(s)` URL.

**Why is `anymd` faster than Docling, Marker, or MarkItDown?**
`anymd` is compiled native Rust (`anymd-core`, `anymd-pdf`, `anymd-formats`) that parses XML/ZIP and PDF glyph coordinates in parallel page by page without loading heavy Python ML frameworks for native text documents.

**Is the core MCP server free forever, or gated behind Pro?**
Reading, outlining, searching, OCR, transcripts, the CLI, and all four core MCP tools (`read`, `outline`, `search`, `inspect`) remain 100% free and MIT licensed. Only offline `cite_check` and verifiable video frame/timeline inspection require [anymd Pro](/pricing?entry_surface=seo_pdf-and-office-to-markdown-mcp) ($29 one-time).

Need to verify PDF quotes or cite exact video frames? **[See Anymd Pro](/pricing?entry_surface=seo_pdf-and-office-to-markdown-mcp)**.
