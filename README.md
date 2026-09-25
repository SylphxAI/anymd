<div align="center">

<img src="docs/public/logo.svg" alt="anymd" width="96" height="96" />

# anymd

**Any file → clean Markdown for AI agents.**

PDF, Word, PowerPoint, Excel, EPUB, HTML, images, audio/video. A fast Rust engine running on your machine, available as an MCP server and a CLI. No API key.

[![npm](https://img.shields.io/npm/v/@sylphx/anymd?style=flat-square&labelColor=0a0d07&color=c3f53c)](https://www.npmjs.com/package/@sylphx/anymd)
[![downloads](https://img.shields.io/npm/dm/@sylphx/anymd?style=flat-square&labelColor=0a0d07&color=c3f53c)](https://www.npmjs.com/package/@sylphx/anymd)
[![stars](https://img.shields.io/github/stars/SylphxAI/anymd?style=flat-square&labelColor=0a0d07&color=c3f53c)](https://github.com/SylphxAI/anymd/stargazers)
[![MCP registry](https://img.shields.io/badge/MCP-io.github.SylphxAI%2Fanymd-c3f53c?style=flat-square&labelColor=0a0d07)](https://registry.modelcontextprotocol.io/v0/servers?search=anymd)
[![license](https://img.shields.io/badge/license-MIT-c3f53c?style=flat-square&labelColor=0a0d07)](LICENSE)

[Install](#install) · [Benchmarks](#benchmarks) · [Tools](#mcp-tools) · [CLI](#cli) · [Formats](#formats) · [Docs](https://sylphxai.github.io/anymd/)

<sub>Formerly **pdf-reader-mcp** / **Citra**. `@sylphx/pdf-reader-mcp` and `@sylphx/citra` still install and run anymd.</sub>

<img src="docs/public/demo.svg" alt="anymd converting a PDF, searching a folder, and reading a spreadsheet" width="760" />

</div>

## Why anymd

- **Fast.** Native Rust converts in parallel, page by page. A 15-page paper converts in about **0.1 s**. That is 20× faster than MarkItDown and 500× faster than docling, with every table intact.
- **Accurate.** A layout engine rebuilds words from glyph gaps, puts two-column papers in reading order, and recovers tables, including borderless ones. The text stays exactly as printed, with no glued words and no scrambled columns.
- **Lean on tokens.** Pages come back as Markdown with `<!-- page 3 -->` citation anchors, a small front-matter header, and compact tables. A token budget and a cursor keep large documents within your agent's context.
- **Every format, one call.** One tool reads every format listed below. It also accepts web URLs and whole directories, and `search` looks across all of them.
- **Local and private.** Nothing is uploaded. OCR and transcripts use local tools you already have (tesseract, ffmpeg, whisper.cpp), and only when they are installed.

## Install

Every MCP client runs the same command, `npx -y @sylphx/anymd`. Node 18+ is the only requirement; npm installs the native binary for your platform.

<details open>
<summary><b>Claude Code</b></summary>

```bash
claude mcp add anymd -- npx -y @sylphx/anymd
```
</details>

<details>
<summary><b>Codex</b></summary>

```bash
codex mcp add anymd -- npx -y @sylphx/anymd
```

or in `~/.codex/config.toml`:

```toml
[mcp_servers.anymd]
command = "npx"
args = ["-y", "@sylphx/anymd"]
```
</details>

<details>
<summary><b>Cursor</b></summary>

[![Add to Cursor](https://cursor.com/deeplink/mcp-install-dark.svg)](https://cursor.com/en/install-mcp?name=anymd&config=eyJjb21tYW5kIjoibnB4IiwiYXJncyI6WyIteSIsIkBzeWxwaHgvYW55bWQiXX0=)

or in `.cursor/mcp.json`:

```json
{ "mcpServers": { "anymd": { "command": "npx", "args": ["-y", "@sylphx/anymd"] } } }
```
</details>

<details>
<summary><b>VS Code</b></summary>

```bash
code --add-mcp '{"name":"anymd","command":"npx","args":["-y","@sylphx/anymd"]}'
```

or in `.vscode/mcp.json`:

```json
{ "servers": { "anymd": { "type": "stdio", "command": "npx", "args": ["-y", "@sylphx/anymd"] } } }
```
</details>

<details>
<summary><b>Claude Desktop</b></summary>

Add to `claude_desktop_config.json` (Settings → Developer → Edit Config):

```json
{ "mcpServers": { "anymd": { "command": "npx", "args": ["-y", "@sylphx/anymd"] } } }
```
</details>

<details>
<summary><b>Windsurf, Zed, Cline, and other clients</b></summary>

Any client that speaks MCP over stdio: command `npx`, args `["-y", "@sylphx/anymd"]`. To keep the server inside one folder, add `--allow-dir=/path/to/docs`.
</details>

<details>
<summary><b>CLI only</b></summary>

```bash
npm install -g @sylphx/anymd     # or run it once with: npx -y @sylphx/anymd <file>
```
</details>

## Benchmarks

Twelve real documents (papers, a two-column paper, statistical tables, CJK, a form, a borderless-table invoice, plus DOCX, PPTX, XLSX, EPUB, and a Wikipedia page), run on a GitHub-hosted runner with 4 CPUs:

| | **anymd** | docling | MarkItDown | kreuzberg | pdftotext |
|---|---|---|---|---|---|
| Total time, 12 documents | **0.40 s** | 964 s | 22.5 s | 2.8 s | 0.36 s ¹ |
| Sentences intact (12) | **12** | 11 | 5 | 12 | 12 |
| Table rows recovered (26) | **26** | 25 | 15 | 0 | 0 |
| Reading order correct (5) | **5** | 3 | 3 | 5 | 5 |
| Output tokens (o200k) | **98.6k** | 126.4k | 144.7k | 125.0k | 74.1k ¹ |

<sub>¹ pdftotext reads PDFs only and outputs plain text without tables.</sub>

On the 15-page *Attention Is All You Need* paper, anymd takes **0.13 s**, docling 76 s, and MarkItDown 3.0 s, and MarkItDown glues the words together ("dominantsequencetransductionmodels"). On the Wikipedia article, anymd's main-content extraction uses **21.7k tokens**; docling uses 37.6k, kreuzberg 51.7k, and MarkItDown 54.6k.

Each tool runs as a fresh process, and every number is the median of 3 runs (docling runs once, after its models are warmed up). Tokens are counted with `o200k_base`. **Sentences intact** counts reference sentences that come out verbatim; glued words or split columns fail the check. **Table rows** counts ground-truth rows that come out as one Markdown table row with the cells in order. The method, corpus, ground truth, raw results, and scripts are in [`bench/`](bench/), and the [Benchmark workflow](.github/workflows/benchmark.yml) re-runs everything on GitHub-hosted runners.

The official `@modelcontextprotocol/server-pdf` is left out of the table because it has no headless text path. It renders PDFs in an interactive viewer, and its `read_pdf_bytes` tool returns base64-encoded bytes.

## MCP tools

anymd exposes three tools.

| Tool | Use it to | Key arguments |
|---|---|---|
| **`read`** | Turn a file, URL, or folder into Markdown | `source`, `pages` (`"1-5,8"`), `max_tokens` (default 20000), `cursor`, `ocr`, `transcript` |
| **`search`** | Find text across files, folders, and URLs | `query`, `sources`, `mode` (`auto` · `literal` · `ranked`), `glob`, `max_results` |
| **`inspect`** | Go deeper on a PDF | `operation`: `render_page`, `extract_regions`, `ocr_pages`, `structure` (JSON with geometry), `compare`, `inspect` |

A `read` answer looks like this:

```markdown
---
source: papers/attention.pdf
title: Attention Is All You Need
pages: 15
showing: pages 1-9
---

<!-- page 1 -->

# Attention Is All You Need
…

<!-- page 8 -->

|Model|BLEU EN-DE|BLEU EN-FR|
|-|-|-|
|Transformer (big)|28.4|41.8|
…

<!-- Stopped at the 20000-token budget. Continue with cursor: "10", or pick pages, or raise max_tokens. -->
```

`search` answers with one line per hit:

```markdown
5 matches for "masked language model" (2 files, 31 sections searched)

### papers/bert.pdf (5)
- p.1: …by using a “**masked language model**” (MLM) pre-training objective, inspired by the Cloze task…
- p.2: …In addition to the **masked language model**, we also use a “next sentence prediction” task…
```

If nothing matches exactly, `search` falls back to BM25-ranked passages, so a question like "how does bidirectional pretraining work" still finds the right page.

<sub>The pdf-reader-mcp tool names (`read_pdf`, `search_pdf`, `pdf_evidence`, `pdf_compare`) still work for this major version. They no longer appear in tools/list.</sub>

## CLI

The same binary is a command-line converter, like MarkItDown but much faster:

```bash
anymd report.pdf > report.md                 # a file
anymd deck.pptx notes.docx budget.xlsx        # several files, each with a header
anymd https://example.com/article            # a web page (main content only)
cat scan.png | anymd - --ocr                 # stdin, with OCR
anymd paper.pdf --pages 1-3 --max-tokens 4000
anymd search "indemnification" contracts/ --glob '*.pdf'
anymd doctor                                 # lists the optional tools anymd found
```

Run with no arguments from an MCP client (piped stdin), or as `anymd mcp`, and it serves MCP over stdio.

## Formats

| Input | What you get |
|---|---|
| **PDF** | Reading-order Markdown: headings, paragraphs, lists, tables, sub/superscripts, `<!-- page N -->` markers, bookmarks as an outline. Running headers and page numbers are removed. Image-only pages are OCR'd when `tesseract` is installed. |
| **Word** `.docx` | Headings, bold/italic, links, nested lists, tables with merged cells, footnotes, equations as LaTeX |
| **PowerPoint** `.pptx` | One section per slide in deck order, titles, bullets, tables, chart data, speaker notes |
| **Excel** `.xlsx .xls .ods` · **CSV/TSV** | One table per sheet, dates as ISO strings, capped at 2,000 rows per sheet |
| **EPUB** | One section per chapter in spine order, plus title and author |
| **HTML** and **URLs** | The main article only: navigation, cookie banners, and sidebars are dropped. Relative links are resolved, and code keeps its language. |
| **Markdown, text, JSON** | Returned unchanged, with pagination |
| **Images** | Dimensions and EXIF (camera, date, GPS), plus OCR text when `tesseract` is installed |
| **Audio / video** | Duration, streams, chapters, embedded and sidecar subtitles (via `ffprobe`/`ffmpeg`). Local whisper.cpp transcript with `transcript: true`. |

## How it works

For PDFs, anymd reads glyph positions rather than text runs. Glyphs are grouped into lines by baseline, which tolerates super- and subscripts. Word spaces come from the gaps between glyphs, measured against the font size and adjusted for letter tracking. A column-aware XY cut finds gutters between running text. Rows whose cells line up become pipe tables. Pages are processed in parallel and isolated from each other, so one malformed page never fails the whole document. The other formats are parsed natively in Rust (zip/XML, calamine, html5ever); no Python, LibreOffice, or cloud service is involved.

## Security

- Local-first: documents never leave your machine unless you pass a URL, and even then only that URL is fetched.
- URL fetches block private and loopback addresses, and every redirect hop is checked again, pinned to its resolved address.
- `--allow-dir=<path>` (repeatable) or `MCP_PDF_ALLOWED_DIRS` confines the server to the directories you list.
- External tools (tesseract, ffprobe, whisper.cpp) are optional. anymd runs them without a shell, with a timeout and an output cap.

See [SECURITY.md](SECURITY.md) to report a vulnerability.

## Star history

[![Star History Chart](https://api.star-history.com/svg?repos=SylphxAI/anymd&type=Date)](https://star-history.com/#SylphxAI/anymd&Date)

## License

MIT © [Sylphx](https://sylphx.com)
