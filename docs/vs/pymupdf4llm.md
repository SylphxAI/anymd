---
title: "anymd vs PyMuPDF4LLM: MIT Rust MCP alternative to AGPL Python"
titleTemplate: false
description: "anymd vs PyMuPDF4LLM: MIT-licensed Rust MCP server and CLI for PDF, Office and media vs AGPL Python PDF library. Free MIT core or US$29 once."
image: https://anymd.sylphx.com/og-image.png
---

<p class="pro-trust">Comparison · MIT Rust binary &amp; MCP vs AGPL-3.0 Python library</p>

# anymd vs PyMuPDF4LLM

Both tools run locally on a CPU and turn PDFs into Markdown for LLMs without an API key. [PyMuPDF4LLM](https://github.com/pymupdf/RAG) is a fast Python library from Artifex Software built on PyMuPDF (MuPDF), distributed under the **AGPL-3.0** copyleft licence (with paid commercial licences available from Artifex). anymd is one **MIT-licensed** native Rust binary—both a CLI and a built-in MCP server—that handles PDFs alongside Word, PowerPoint, Excel/CSV, EPUB, HTML/URLs, local OCR, and audio/video with no Python runtime. On the 38 public documents of [AgentDocBench](/guide/benchmarks?entry_surface=seo_vs-pymupdf4llm), anymd scores **96.4** overall (**92.2** table cells F1) in **11.5 s** on 4 CPUs (**0.87 s** across the 20 born-digital PDFs).

<p>
  <a class="pro-buy" href="/anymd/guide/getting-started?entry_surface=seo_vs-pymupdf4llm">Install anymd free</a>
  &nbsp;&nbsp;
  <a href="/anymd/pricing?entry_surface=seo_vs-pymupdf4llm">Get Pro (US$29 once)</a>
</p>

```bash
npx -y @sylphx/anymd setup --client cursor,claude-code,codex
```

## At a glance

| | anymd | PyMuPDF4LLM |
|---|---|---|
| What it is | One native Rust binary: CLI and MCP server | Python package built on PyMuPDF / MuPDF |
| Licence | **MIT** (permissive; use in proprietary apps and SaaS without source disclosure) | **AGPL-3.0** (strong copyleft; closed-source or networked commercial apps require an Artifex commercial licence) |
| Install | `npx -y @sylphx/anymd setup`, Homebrew, Scoop, Cargo or Docker | `pip install pymupdf4llm` (Python environment + MuPDF native wheel) |
| MCP tools | Built in: `read`, `outline`, `search`, `inspect` | Separate `pymupdf4llm-mcp` package |
| Formats | PDF, Word (`.docx`), PowerPoint (`.pptx`), Excel (`.xlsx`, `.xls`, `.ods`, CSV), EPUB, HTML/URLs, images (OCR), audio and video | PDF and MuPDF-supported document formats (XPS, EPUB, FB2, CBZ, SVG; Office via separate extras/conversion) |
| Scanned PDFs | Local OCR with tesseract by default, or local doc-VLM models via `anymd setup ocr` | Optional OCR via Tesseract / PyMuPDF Layout |
| Output | Markdown with `<!-- page N -->` anchors; JSON with geometry through `inspect` | Markdown, page-chunked dictionaries, JSON and extracted images |
| Python RAG loaders | Python wheel prepared; CLI/MCP works from any language today | Direct Python API with LangChain and LlamaIndex helpers |
| Local benchmark baseline | **96.4** on AgentDocBench (38 docs in **11.5 s**; **0.87 s** on 20 born-digital PDFs; **92.2** table cells F1) | Not yet in AgentDocBench (local adapter welcome via PR) |
| Price | Free (`$0`), MIT. Optional [anymd Pro](/pricing?entry_surface=seo_vs-pymupdf4llm), **US$29** once | Free under AGPL-3.0; paid commercial licence from Artifex Software for closed-source / SaaS use |

## Local benchmark baseline and honest scope

[AgentDocBench](/guide/benchmarks?entry_surface=seo_vs-pymupdf4llm) tests document converters on 38 public documents across 12 categories on GitHub-hosted 4-CPU runners. anymd 8.2.0 scores **96.4** overall (**97.7** verbatim sentences, **98.8** reading order, **92.2** table cells F1) and finishes all **38/38** documents in **11.5 s** (**0.87 s** on the 20 born-digital PDFs, **368,659** output tokens).

**PyMuPDF4LLM is not yet in AgentDocBench.** Rather than guessing or citing third-party charts, we publish only our committed repository results here. Because PyMuPDF4LLM runs locally on CPU, anyone can add a `bench/adapters/pymupdf4llm.py` adapter and [submit it to AgentDocBench](/guide/benchmarks#submit-a-tool) to measure both tools side by side on the same 38 files.

## Why agent builders pick anymd over PyMuPDF4LLM

### Permissive MIT licence—no AGPL copyleft or commercial licensing negotiations

AGPL-3.0 requires disclosing the complete source code of any networked service or proprietary product that links PyMuPDF4LLM, unless you purchase a commercial licence from Artifex. anymd's core binary and Rust crates (`anymd-core`, `anymd-pdf`, `anymd-formats`) are **MIT licensed**, so startups, SaaS backends and enterprise teams can ship it in closed-source products immediately at **US$0**.

### One binary for every office format, not just PDFs

Real agent folders mix PDFs with `.docx` specs, `.pptx` decks, `.xlsx` models, `.csv` exports, web pages and recordings. anymd parses all of them natively in Rust with zero Python virtual environments and zero LibreOffice installs, while still matching C-speed PDF extraction (`0.87 s` across 20 born-digital PDFs).

### Built-in MCP server ready in one command

`npx -y @sylphx/anymd setup` configures Claude Code, Codex, Cursor, VS Code, Claude Desktop, Windsurf and Gemini CLI at once. Agents get `outline` for heading trees, token-budgeted `read` with `<!-- page N -->` anchors and pagination cursors, folder-wide `search`, and `inspect` for bounding boxes—confined to approved folders with `--allow-dir`.

### Optional offline citation proof for US$29 once

If your agent must prove a PDF quote sits on the cited page or pull the exact decoded video frame behind a claim, [anymd Pro](/pricing?entry_surface=seo_vs-pymupdf4llm) adds `cite_check` and SHA-256 verifiable video timelines for a single **US$29** payment—no annual seat renewal, no server call, valid offline for 100 years.

## When PyMuPDF4LLM is the better fit

- You need an in-process Python library today with ready-made LangChain or LlamaIndex document loaders (`pymupdf4llm.LlamaMarkdownReader`), and AGPL-3.0 compliance or an Artifex commercial licence is already in place.
- Your workflow relies on PyMuPDF's lower-level PDF manipulation APIs (annotations, redaction, rendering, splitting or merging PDFs) in the same Python process.
- You already run `pymupdf4llm` inside a pure-Python RAG ingestion service and only need PDFs.

## Switch in one line

```python
# PyMuPDF4LLM (Python, AGPL-3.0 / commercial licence)
import pymupdf4llm
md = pymupdf4llm.to_markdown("report.pdf")
```

```bash
# anymd (native Rust binary, MIT licence, CLI + MCP)
anymd report.pdf -o report.md
```

For MCP clients, replace `pymupdf4llm-mcp` with `npx -y @sylphx/anymd`, or run `npx -y @sylphx/anymd setup` to configure every client. With Docker:

```bash
docker run --rm -v "$PWD:/data" ghcr.io/sylphxai/anymd report.pdf > report.md
```

More install options are in [Getting started](/guide/getting-started?entry_surface=seo_vs-pymupdf4llm).

## Check what your agent cites with anymd Pro

Converting a file is free. When an agent quotes a PDF or a video, [anymd Pro](/pricing?entry_surface=seo_vs-pymupdf4llm) takes you back to the source:

- **Cite-check** compares up to 100 quotes per request with the page and area each one cites, and reports whether it matches, doesn't match, or needs more evidence. It uses the text and its position on the page, not an AI opinion.
- **Video timelines and frames** show scenes, chapters and subtitle or transcript timings, and the frame behind a cited moment with its actual decoded timestamp and a SHA-256 hash of the image.

Pay once, **US$29** or the local price in 16 currencies, for a 100-year licence for one person. Pro works offline, with no subscription and no account. Buy on the web at [anymd Pro](/pricing?entry_surface=seo_vs-pymupdf4llm) or run `anymd pro buy` in your terminal.

<p>
  <a class="pro-buy" href="/anymd/pricing?entry_surface=seo_vs-pymupdf4llm">See anymd Pro</a>
</p>

## FAQ

**What is the licence difference between anymd and PyMuPDF4LLM?**
anymd is MIT licensed, allowing commercial and closed-source SaaS use with no copyleft obligation. PyMuPDF4LLM and PyMuPDF are licensed under AGPL-3.0, which requires disclosing application source code for networked or proprietary distribution unless you purchase a commercial licence from Artifex Software.

**Does anymd need Python like PyMuPDF4LLM?**
No. anymd ships as a single native Rust binary via npm (`npx -y @sylphx/anymd`), Homebrew, Scoop, Cargo or Docker, with no Python environment or MuPDF installation required.

**Is PyMuPDF4LLM measured on AgentDocBench?**
Not yet. AgentDocBench results are published only for tools measured on the same 38 public documents in `bench/results/`. Anyone can add a PyMuPDF4LLM adapter in `bench/adapters/` and open a pull request to benchmark it on the same runner.

**Can anymd convert Word, PowerPoint and Excel files too?**
Yes. One `anymd` call handles PDF, Word (`.docx`), PowerPoint (`.pptx`), Excel and spreadsheets (`.xlsx`, `.xls`, `.ods`, CSV/TSV), EPUB, HTML and URLs, images with local OCR, and audio/video.

**Is anymd a drop-in replacement for PyMuPDF4LLM in Python?**
From the command line or MCP clients, yes: `anymd report.pdf -o report.md` produces clean Markdown with `<!-- page N -->` anchors and `npx -y @sylphx/anymd setup` configures MCP clients directly. Code importing `pymupdf4llm` in-process in Python cannot swap imports yet until anymd's Python wheel is published on PyPI.

**Is anymd free?**
Yes. anymd is MIT licensed, including the CLI, the MCP server, OCR and transcripts. anymd Pro is an optional one-time purchase, US$29 or the local price in 16 currencies, that adds PDF cite-check and video timelines and frames.

<p class="cit-fine">PyMuPDF and PyMuPDF4LLM are projects of Artifex Software; anymd is not affiliated with or endorsed by Artifex Software. anymd benchmark figures come from the AgentDocBench results committed in the anymd repository. PyMuPDF4LLM licensing and feature details come from its public documentation and repository.</p>
