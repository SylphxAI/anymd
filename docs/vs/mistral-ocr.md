---
title: "anymd vs Mistral OCR: local Rust MCP alternative to cloud OCR"
titleTemplate: false
description: "anymd vs Mistral OCR: local Rust MCP server and CLI with no API key or uploads vs Mistral's metered cloud OCR API. Free MIT core or US$29 once."
image: https://anymd.sylphx.com/og-image.png
---

<p class="pro-trust">Comparison · Local Rust MCP &amp; CLI vs metered Mistral OCR API</p>

# anymd vs Mistral OCR

Both tools extract Markdown from PDFs and images for LLMs. [Mistral OCR](https://docs.mistral.ai/capabilities/OCR/basic_ocr/) (`mistral-ocr-latest`) is Mistral AI's hosted vision-OCR API: you send documents to Mistral's cloud with an API key and pay per 1,000 pages. anymd is one native Rust binary, MIT licensed, that parses born-digital PDFs, Office files, spreadsheets, EPUB, HTML, local OCR scans, and audio/video directly on your machine with no API key, no uploads, and built-in MCP tools (`read`, `outline`, `search`, `inspect`). On the 38 public documents of [AgentDocBench](/guide/benchmarks?entry_surface=seo_vs-mistral-ocr), anymd scores **96.4** overall (**92.2** table cells F1) in **11.5 s** on 4 CPUs.

<p>
  <a class="pro-buy" href="/anymd/guide/getting-started?entry_surface=seo_vs-mistral-ocr">Install anymd free</a>
  &nbsp;&nbsp;
  <a href="/anymd/pricing?entry_surface=seo_vs-mistral-ocr">Get Pro (US$29 once)</a>
</p>

```bash
npx -y @sylphx/anymd setup --client cursor,claude-code,codex
```

## At a glance

| | anymd | Mistral OCR |
|---|---|---|
| What it is | One local native Rust binary: CLI and MCP server | Hosted cloud document OCR API (`mistral-ocr-latest` / OCR 4 and OCR 3) |
| Where files run | On your machine, offline by default; nothing is uploaded | Uploaded to Mistral AI's cloud API (or fetched by URL) |
| API key and account | None | Mistral API key and billing account required |
| Born-digital PDFs | Reads exact PDF glyph coordinates and tables directly in milliseconds (`0.87 s` for 20 PDFs) | Runs every page through cloud vision OCR at the same per-page rate |
| Scanned PDFs and images | Local OCR via installed tesseract, or pinned local doc-VLM models after `anymd setup ocr` | Cloud vision OCR model with strong scan, multilingual and formula extraction |
| Non-PDF formats | Native parsers for Word, PowerPoint, Excel/CSV, EPUB, HTML/URLs, audio and video | PDFs, images and document URLs supported by the OCR endpoint |
| MCP tools | Built in: `read`, `outline`, `search`, `inspect` | API endpoint only; requires a separate MCP wrapper |
| Local benchmark baseline | **96.4** on AgentDocBench (38 docs in **11.5 s** on 4 CPUs; **92.2** table cells F1) | Hosted cloud API; not measured on AgentDocBench |
| Licence | MIT | Proprietary hosted API |
| Price | Free (`$0`), MIT. Optional [anymd Pro](/pricing?entry_surface=seo_vs-mistral-ocr), **US$29** once | Metered per 1,000 pages: US$2.00 (OCR 3) to US$4.00 (`mistral-ocr-latest`), 50% off via Batch API, extra for JSON annotations |

## Local benchmark baseline and honest scope

[AgentDocBench](/guide/benchmarks?entry_surface=seo_vs-mistral-ocr) evaluates local converters on 38 public documents across 12 categories on GitHub-hosted 4-CPU runners with no API keys or remote uploads. anymd 8.2.0 scores **96.4** overall (**97.7** verbatim sentences, **98.8** reading order, **92.2** table cells F1), converts all **38/38** documents in **11.5 s** (**0.87 s** on the 20 born-digital PDFs), and returns **368,659** tokens.

**Mistral OCR is not in AgentDocBench** because it is a hosted cloud API requiring credentials and file uploads. We also publish anymd's scan-only limitations openly: on OmniDocBench v1.6 page images, anymd's default English Tesseract path scores **21.4** (and **77.9** on AgentDocBench's three scanned PDFs). When your corpus is mostly degraded scans, handwriting or dense photographed equations and cloud upload is allowed, a hosted vision OCR model like Mistral OCR is built for that workload.

## Why agent builders pick anymd over Mistral OCR

### Exact text extraction on born-digital PDFs—no OCR hallucination or per-page bill

Most developer, legal, financial and research PDFs already contain an embedded text layer. Sending born-digital PDFs to a vision OCR API burns US$2–US$4 per 1,000 pages, adds network round-trips, and risks character substitution on dense tables or part numbers. anymd reads the PDF's exact glyph positions natively in Rust, keeping columns in order (**98.8**) and tables intact (**92.2** table cells F1) in milliseconds at **US$0** per page.

### Zero uploads, zero API keys, full offline privacy

Documents stay on your laptop, private runner or air-gapped server. anymd needs no account, sends no telemetry, and locks MCP access to chosen directories via `--allow-dir`.

### One tool for Office, spreadsheets, web pages, audio and video—plus MCP

Agents rarely read only scanned PDFs. anymd converts `.docx`, `.pptx`, `.xlsx`, `.csv`, `.epub`, `.html`, URLs, images, and audio/video through the same `read` tool, with `outline` navigation, token-budgeted pagination (`cursor`), folder-wide `search`, and `<!-- page N -->` citation anchors.

### Free MIT core, or US$29 once for verifiable Pro evidence

Everything needed for conversion, MCP, local OCR and local transcripts is free and MIT licensed. If your agent needs deterministic PDF `cite_check` or SHA-256 verifiable video frames, [anymd Pro](/pricing?entry_surface=seo_vs-mistral-ocr) costs **US$29** once—less than a single 8,000-page Mistral OCR 4 run—and works offline for 100 years.

## When Mistral OCR is the better fit

- Your documents are predominantly low-quality scans, camera photos, handwriting, or complex display math where cloud vision OCR beats local Tesseract or CPU doc-VLM inference.
- You process tens of thousands of scanned pages asynchronously through Mistral's Batch API (50% discount) and cloud processing is permitted under your data policy.
- You want structured JSON-schema annotations directly from page images in one API call.

## Switch in one line

```python
# Mistral OCR (uploads file, requires MISTRAL_API_KEY, billed per 1,000 pages)
from mistralai import Mistral
res = Mistral(api_key=api_key).ocr.process(model="mistral-ocr-latest", document={"type": "document_url", "document_url": url})
```

```bash
# anymd (runs locally, no API key, free MIT)
anymd report.pdf -o report.md
```

For MCP clients, run `npx -y @sylphx/anymd setup` to wire Claude Code, Codex, Cursor, VS Code, Claude Desktop, Windsurf and Gemini CLI, or run in Docker:

```bash
docker run --rm -v "$PWD:/data" ghcr.io/sylphxai/anymd report.pdf > report.md
```

More install options are in [Getting started](/guide/getting-started?entry_surface=seo_vs-mistral-ocr).

## Check what your agent cites with anymd Pro

Converting a file is free. When an agent quotes a PDF or a video, [anymd Pro](/pricing?entry_surface=seo_vs-mistral-ocr) takes you back to the source:

- **Cite-check** compares up to 100 quotes per request with the page and area each one cites, and reports whether it matches, doesn't match, or needs more evidence. It uses the text and its position on the page, not an AI opinion.
- **Video timelines and frames** show scenes, chapters and subtitle or transcript timings, and the frame behind a cited moment with its actual decoded timestamp and a SHA-256 hash of the image.

Pay once, **US$29** or the local price in 16 currencies, for a 100-year licence for one person. Pro works offline, with no subscription and no account. Buy on the web at [anymd Pro](/pricing?entry_surface=seo_vs-mistral-ocr) or run `anymd pro buy` in your terminal.

<p>
  <a class="pro-buy" href="/anymd/pricing?entry_surface=seo_vs-mistral-ocr">See anymd Pro</a>
</p>

## FAQ

**Does anymd require an API key or send files to the cloud like Mistral OCR?**
No. anymd runs locally on your machine under the MIT licence with no API key and no file uploads. Network calls happen only when you pass an explicit http(s) URL or explicitly download a local OCR or ASR model.

**How does anymd compare with Mistral OCR on pricing?**
anymd's local converter, CLI, MCP server, OCR and transcripts are free and MIT licensed. Optional anymd Pro is US$29 once (or the local price in 16 currencies) for a 100-year offline licence. Mistral OCR charges per 1,000 pages (US$2.00 for OCR 3 and US$4.00 for mistral-ocr-latest, with a 50% batch discount).

**Can anymd OCR scanned PDFs without Mistral OCR?**
Yes. anymd runs local Tesseract OCR automatically on image-only pages, and `anymd setup ocr` installs pinned local document-VLM weights (PaddleOCR-VL-1.6 and PP-DocLayoutV3) for offline tables and formulas. On heavily degraded scans or handwriting where cloud upload is allowed, Mistral OCR can still be faster and more accurate.

**Why isn't Mistral OCR on the AgentDocBench leaderboard?**
AgentDocBench benchmarks local open-source tools on GitHub-hosted 4-CPU runners without API keys or external uploads. Mistral OCR is a proprietary cloud API, so we do not fabricate a benchmark score for it.

**Is anymd faster than Mistral OCR on normal PDFs?**
On born-digital PDFs, anymd reads the embedded text layer directly in native Rust with zero network latency: all 20 born-digital PDFs in AgentDocBench convert in 0.87 seconds total on 4 CPUs.

**Is anymd free?**
Yes. anymd is MIT licensed, including the CLI, the MCP server, OCR and transcripts. anymd Pro is an optional one-time purchase, US$29 or the local price in 16 currencies, that adds PDF cite-check and video timelines and frames.

<p class="cit-fine">Mistral OCR is a product of Mistral AI; anymd is not affiliated with or endorsed by Mistral AI. anymd benchmark figures come from the AgentDocBench and OmniDocBench results committed in the anymd repository. Mistral OCR pricing and model details come from Mistral AI's public documentation and may change over time.</p>
