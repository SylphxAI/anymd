<div align="center">

<img src="docs/public/og-image.png" alt="anymd — any file → clean Markdown for AI agents" width="820" />

<h1 hidden>anymd</h1>

<!-- generated:lead -->
PDF, Word, PowerPoint, Excel, EPUB, HTML and web pages, images (OCR), audio and video (metadata, subtitles, transcripts). A fast Rust MCP server and CLI that runs on your machine. No API key.
<!-- /generated:lead -->

[![npm](https://img.shields.io/npm/v/@sylphx/anymd?style=flat-square&labelColor=0a0d07&color=c3f53c)](https://www.npmjs.com/package/@sylphx/anymd)
[![downloads](https://img.shields.io/npm/dm/@sylphx/anymd?style=flat-square&labelColor=0a0d07&color=c3f53c)](https://www.npmjs.com/package/@sylphx/anymd)
[![stars](https://img.shields.io/github/stars/SylphxAI/anymd?style=flat-square&labelColor=0a0d07&color=c3f53c)](https://github.com/SylphxAI/anymd/stargazers)
[![MCP registry](https://img.shields.io/badge/MCP-io.github.SylphxAI%2Fanymd-c3f53c?style=flat-square&labelColor=0a0d07)](https://registry.modelcontextprotocol.io/v0/servers?search=anymd)
[![license](https://img.shields.io/badge/license-MIT-c3f53c?style=flat-square&labelColor=0a0d07)](LICENSE)

[Install](#install) · [Benchmarks](#benchmarks) · [Tools](#mcp-tools) · [CLI](#cli) · [Formats](#formats) · [Docs](https://sylphxai.github.io/anymd/)

<!-- generated:formerly -->
<sub>Formerly **pdf-reader-mcp**. [Migrating from pdf-reader-mcp](https://sylphxai.github.io/anymd/guide/migration)</sub>
<!-- /generated:formerly -->

<img src="docs/public/demo.gif" alt="Real terminal session: anymd converts a PDF page with its table, searches a folder, reads a spreadsheet, then Claude Code answers from the PDF through the anymd MCP server" width="820" />

<sub>A real, unedited terminal recording (asciinema + agg, <a href="bench/demo">script</a>). The last command is Claude Code answering from the PDF through the anymd MCP server.</sub>

</div>

## Why anymd

<!-- fast:start -->
- **Fast.** Native Rust converts in parallel, page by page. On the 19 benchmark documents every tool converted, anymd takes **12.1 s** in total; docling 1,723.3 s (142×), markitdown 45.4 s (4×), marker 5,256.3 s (434×).
<!-- fast:end -->
- **Accurate.** A layout engine rebuilds words from glyph gaps, puts two-column papers in reading order, and recovers tables, including borderless ones. The text stays exactly as printed, with no glued words and no scrambled columns.
- **Lean on tokens.** Pages come back as Markdown with `<!-- page 3 -->` citation anchors, a small front-matter header, and compact tables. A token budget and a cursor keep large documents within your agent's context.
- **Every format, one call.** One tool reads every format listed below. It also accepts web URLs and whole directories, and `search` looks across all of them.
- **Local and private.** Nothing is uploaded. OCR and transcripts use local tools you already have (tesseract, ffmpeg, whisper.cpp), and only when they are installed.

## Install

Add anymd to every MCP client on your machine (Claude Code, Codex, Cursor, VS Code, Claude Desktop, Windsurf, Gemini CLI) with one command:

```bash
npx -y @sylphx/anymd setup     # --dry-run to preview, --remove to undo
```

Or add it by hand: every MCP client runs the same command, `npx -y @sylphx/anymd`. Node 18+ is the only requirement; npm installs the native binary for your platform.

<details open>
<summary><b>Claude Code</b></summary>

```bash
claude mcp add anymd -- npx -y @sylphx/anymd
```

Or as a plugin, with the anymd skill: `/plugin marketplace add SylphxAI/anymd`, then `/plugin install anymd@anymd`.
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

[AgentDocBench](docs/guide/benchmarks.md) is an open benchmark for document → Markdown conversion for agents: license-clean documents in 12 categories (math papers, two-column papers, financial tables, forms, scans, CJK, slides, spreadsheets, Word, EPUB, HTML), scored on verbatim sentences, text F1, reading order, and table cells, with time and output tokens. Every tool runs on the same kind of GitHub-hosted runner (4 CPUs):

<!-- headline:start -->

| | docling | **anymd** | kreuzberg | unstructured | markitdown | marker | pdftotext |
|---|---|---|---|---|---|---|---|
| Overall score | 93.0 | 90.4 | 81.7 | 81.2 | 76.8 | 71.0 | 42.2 |
| Table cells F1 | 89.9 | 75.5 | 38.4 | 38.4 | 57.2 | 60.9 | 0.0 |
| Reading order | 94.4 | 96.2 | 96.8 | 93.9 | 85.5 | 76.8 | 52.0 |
| Docs converted | 38/38 | 38/38 | 38/38 | 38/38 | 38/38 | 30/38 | 23/38 |
| Time, all docs | 2,432.4 s | 30.0 s | 16.0 s | 346.5 s | 75.6 s | 7,104.5 s | 0.90 s |

<!-- headline:end -->

The generated leaderboard, per-category scores (including where anymd loses), and method are in the [benchmark guide](docs/guide/benchmarks.md). The corpus, ground truth, adapters, and raw results are in [`bench/`](bench/), and the [Benchmark workflow](.github/workflows/benchmark.yml) reruns everything; new tools can join with a single adapter file.

## MCP tools

anymd exposes three tools.

| Tool | Use it to | Key arguments |
|---|---|---|
| **`read`** | Turn a file, URL, or folder into Markdown | `source`, `pages` (`"1-5,8"`), `max_tokens` (default 20000), `cursor`, `ocr`, `transcript`, `download_whisper_model` |
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
| **Audio / video** | Duration, streams, chapters, embedded and sidecar subtitles (via `ffprobe`/`ffmpeg`). Local whisper.cpp transcript with `transcript: true`; `download_whisper_model: true` fetches a verified model on first use. |

## How it works

For PDFs, anymd reads glyph positions rather than text runs. Glyphs are grouped into lines by baseline, which tolerates super- and subscripts. Word spaces come from the gaps between glyphs, measured against the font size and adjusted for letter tracking. A column-aware XY cut finds gutters between running text. Rows whose cells line up become pipe tables. Pages are processed in parallel and isolated from each other, so one malformed page never fails the whole document. The other formats are parsed natively in Rust (zip/XML, calamine, html5ever); no Python, LibreOffice, or cloud service is involved.

## Security

- Local-first: documents never leave your machine unless you pass a URL, and even then only that URL is fetched.
- URL fetches block private and loopback addresses, and every redirect hop is checked again, pinned to its resolved address.
- `--allow-dir=<path>` (repeatable) or `MCP_PDF_ALLOWED_DIRS` confines the server to the directories you list.
- External tools (tesseract, ffprobe, whisper.cpp) are optional. anymd runs them without a shell, with a timeout and an output cap.

See [SECURITY.md](SECURITY.md) to report a vulnerability.

## Also from Sylphx

<!-- generated:also-from -->
- [**repomap**](https://github.com/SylphxAI/repomap): A map of your codebase for AI agents: code graph, search, call paths and change impact — with an interactive graph UI. Rust MCP server + CLI. Local, no API key, MIT.
- [**lockdocs**](https://github.com/SylphxAI/lockdocs): Exact-version library docs from your lockfile — local, offline, no rate limits.
- [**readme-mark**](https://github.com/SylphxAI/readme-mark): Beautiful README images from one URL — animated banners, shields-compatible badges, typing text, 3000+ tech icons, GitHub stats cards. Free, no token, drop-in for shields / capsule-render / skill-icons / readme-typing-svg / github-readme-stats.
<!-- /generated:also-from -->

## Star history

[![Star History Chart](https://api.star-history.com/svg?repos=SylphxAI/anymd&type=Date)](https://star-history.com/#SylphxAI/anymd&Date)

## License

MIT © [Sylphx](https://sylphx.com)
