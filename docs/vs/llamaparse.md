---
title: "anymd vs LlamaParse: a local LlamaParse alternative with no API key"
titleTemplate: false
description: "anymd vs LlamaParse: local Rust MCP server and CLI with no API key or file uploads vs metered LlamaCloud parsing. Free MIT core or US$29 once."
image: https://anymd.sylphx.com/og-image.png
---

Comparison · Local Rust MCP & CLI vs hosted LlamaCloud API

# anymd vs LlamaParse

Both tools turn PDFs and office documents into clean Markdown for AI agents and RAG pipelines. [LlamaParse](https://developers.llamaindex.ai/python/cloud/llamaparse/) is LlamaIndex's hosted document parsing API on LlamaCloud: you create an account, pass an API key, upload files to LlamaCloud, and pay in per-page credits. anymd is one native Rust binary, MIT licensed, that converts files locally on your machine with no API key, no uploads, and built-in MCP tools (`read`, `outline`, `search`, `inspect`). On the 38 public documents of [AgentDocBench](/docs/benchmarks?entry_surface=seo_vs-llamaparse), anymd scores **96.4** overall (**92.2** table cells F1) and converts all 38 in **11.5 s** on 4 CPUs.

[Install anymd free](/docs?entry_surface=seo_vs-llamaparse)

**[See Anymd Pro (US$29 once)](/pricing?entry_surface=seo_vs-llamaparse)**

```bash
npx -y @sylphx/anymd setup --client cursor,claude-code,codex
```

## At a glance

| | anymd | LlamaParse |
|---|---|---|
| What it is | One local native Rust binary: CLI and MCP server | Hosted document parsing API on LlamaCloud, with Python, TypeScript, Go and Java SDKs |
| Where files run | On your machine, offline by default; nothing is uploaded | Uploaded to LlamaCloud servers for parsing |
| API key and account | None | LlamaCloud account and API key required |
| Install | `npx -y @sylphx/anymd setup`, `pip install anymd` or `uvx anymd` ([PyPI](https://pypi.org/project/anymd/)), Homebrew, Scoop, Cargo or Docker | `pip install llama-cloud` / `llama-parse` and a `LLAMA_CLOUD_API_KEY` |
| MCP tools | Built in: `read`, `outline`, `search`, `inspect` | Not bundled in a single local binary; wired via LlamaCloud integrations or custom wrappers |
| Formats | PDF, Word, PowerPoint, Excel, CSV, EPUB, HTML and URLs, images with local OCR, audio and video | PDF, Word, PowerPoint, Excel, HTML, images and dozens of office formats via LlamaCloud |
| Output | Markdown with `<!-- page N -->` anchors; JSON with geometry through `inspect` | Markdown, plain text, structured JSON, layout bounding boxes and extracted images |
| Local speed and benchmark | **96.4** on AgentDocBench (38 docs in **11.5 s** on 4 CPUs; **0.87 s** on 20 born-digital PDFs; **92.2** table cells F1) | Hosted cloud API; not measured on AgentDocBench |
| Licence | MIT | Proprietary hosted service (open-source SDKs) |
| Price | Free (`$0`), MIT. Optional [anymd Pro](/pricing?entry_surface=seo_vs-llamaparse), **US$29** once | Free tier (10,000 credits/mo); paid tiers from 1 credit/page (US$1.50/1,000 pages Fast) to 3–25 credits/page (US$4.50–US$37.50/1,000 pages) or monthly plans |

## Local benchmark baseline and honest scope

[AgentDocBench](/docs/benchmarks?entry_surface=seo_vs-llamaparse) measures local open-source converters on 38 public documents across 12 categories on GitHub-hosted 4-CPU runners with no API keys or external uploads. On that corpus, anymd 8.2.0 scores **96.4** overall (**97.7** verbatim sentences, **98.8** reading order, **92.2** table cells F1), converts all **38/38** documents in **11.5 s** (**0.87 s** across the 20 born-digital PDFs), and outputs **368,659** tokens.

**LlamaParse is not in AgentDocBench** because it is a hosted API that requires an account, an API key and document uploads rather than a local offline tool. This page compares architecture, privacy, latency, MCP support and cost honestly rather than quoting unverified scores. All raw AgentDocBench outputs for anymd and six local alternatives are public in [`bench/`](https://github.com/SylphxAI/anymd/tree/main/bench).

## Why agent builders pick anymd over LlamaParse

### Private by default, with no API key

Contracts, financials, medical PDFs and internal slide decks never leave your laptop, CI runner or container. anymd makes no network calls unless you explicitly pass an `http(s)` URL or opt in to downloading a local OCR or speech model, and `--allow-dir` confines the MCP server to one folder.

### Predictable cost: free MIT core, or US$29 once

Metered credit tiers add up quickly when agents re-read folders, parse long reports or run in CI. On LlamaParse's pay-as-you-go tiers, 10,000 pages in Cost-effective mode (3 credits/page) costs US$45 and in Agentic mode (10 credits/page) costs US$150—every run unless cached. anymd's converter, CLI, MCP server, local OCR and transcripts are free and MIT licensed forever. When you want offline PDF `cite_check` and SHA-256 verifiable video frames, [anymd Pro](/pricing?entry_surface=seo_vs-llamaparse) is a single **US$29** payment with offline use after activation.

### Millisecond local reads inside your MCP client

Because anymd parses PDF glyph coordinates and Office XML natively in Rust without uploading files or polling cloud jobs, born-digital PDFs return in milliseconds (`0.87 s` total across 20 born-digital PDFs on 4 CPUs). One command—`npx -y @sylphx/anymd setup`—wires Claude Code, Codex, Cursor, VS Code, Claude Desktop, Windsurf and Gemini CLI with `outline`, token-budgeted `read` with `<!-- page N -->` anchors, `search` and `inspect`.

### Verifiable PDF citations and video frames with anymd Pro

Need deterministic quote verification (`cite_check`) or SHA-256 verifiable video frames? [anymd Pro](/pricing?entry_surface=seo_vs-llamaparse) unlocks both offline in the same Rust binary for **US$29** once.

## When LlamaParse is the better fit

- You need multimodal LLM-assisted parsing (Agentic / Agentic Plus tiers) to interpret complex charts, visual diagrams, or heavily degraded handwritten scans and uploading documents to LlamaCloud is approved.
- Your pipeline is already built on LlamaCloud indexes, workflows and enterprise data connectors.
- You want managed cloud caching and enterprise BAA/HIPAA contracts rather than operating local binaries.

## Switch in one line

```python
# LlamaParse (uploads file, requires LLAMA_CLOUD_API_KEY)
from llama_cloud_services import LlamaParse
docs = LlamaParse(result_type="markdown").load_data("report.pdf")
```

```bash
# anymd (runs locally, no API key, free MIT)
pip install anymd
anymd report.pdf -o report.md
# Or run once: uvx anymd report.pdf -o report.md
```

The [PyPI wheel](https://pypi.org/project/anymd/) also includes a [Python API](/docs/python) for scripts that need Markdown.

For MCP agents, run `npx -y @sylphx/anymd setup` to register `anymd` across your local editors and CLI agents, or run it in Docker:

```bash
docker run --rm -v "$PWD:/data" ghcr.io/sylphxai/anymd report.pdf > report.md
```

More install options are in [Getting started](/docs/getting-started?entry_surface=seo_vs-llamaparse).

## Check what your agent cites with anymd Pro

Converting a file is free. When an agent quotes a PDF or a video, [anymd Pro](/pricing?entry_surface=seo_vs-llamaparse) takes you back to the source:

- **Cite-check** compares up to 100 quotes per request with the page and area each one cites, and reports whether it matches, doesn't match, or needs more evidence. It uses the text and its position on the page, not an AI opinion.
- **Video timelines and frames** show scenes, chapters and subtitle or transcript timings, and the frame behind a cited moment with its actual decoded timestamp and a SHA-256 hash of the image.

**One-time US$29 purchase. No subscription.** Sign in, pay securely, then run `anymd login` with the same account to activate Pro on your machine. After activation, Pro works offline.

**[See the recorded PDF quote check](/studio?sample=pdf&entry_surface=seo_vs-llamaparse#studio-cite-heading)** &nbsp;&nbsp; **[See Anymd Pro](/pricing?entry_surface=seo_vs-llamaparse)**

## FAQ

**Does anymd upload my files or require an API key like LlamaParse?**
No. anymd runs locally on your machine under the MIT licence with no account and no API key. Network requests happen only when you pass an explicit http(s) URL or opt in to downloading a local OCR or transcript model.

**How does anymd pricing compare with LlamaParse?**
anymd's converter, CLI, MCP server, OCR and transcripts are free and MIT licensed, with no per-page credits. Optional anymd Pro is a one-time payment of US$29 (or the local price in 16 currencies) with offline use after activation. LlamaParse offers 10,000 free credits per month and charges per page in credits (1 to 25 credits per page depending on tier) or monthly subscriptions.

**Why isn't LlamaParse on the AgentDocBench scoreboard?**
AgentDocBench runs local converters without API keys or cloud uploads on GitHub-hosted 4-CPU runners. LlamaParse is a hosted cloud API, so we do not fabricate a score for it here.

**Can anymd read scanned PDFs without LlamaCloud?**
Yes. anymd reads scanned pages locally with installed tesseract by default, or with pinned local document-OCR models after running `anymd setup ocr`. For degraded charts or handwriting where a hosted multimodal API is acceptable, LlamaParse's Agentic tiers can be a better fit.

**Does anymd work with Claude Code, Cursor and Codex?**
Yes. Run `npx -y @sylphx/anymd setup` to configure Claude Code, Codex, Cursor, VS Code, Claude Desktop, Windsurf and Gemini CLI automatically, or install via Homebrew, Scoop, Cargo or Docker.

**Is anymd free?**
Yes. anymd is MIT licensed, including the CLI, the MCP server, OCR and transcripts. anymd Pro is an optional one-time purchase, US$29 or the local price in 16 currencies, that adds PDF cite-check and video timelines and frames.

LlamaParse and LlamaCloud are products of LlamaIndex; anymd is not affiliated with or endorsed by LlamaIndex. anymd benchmark figures come from the AgentDocBench results committed in the anymd repository. LlamaParse pricing and tier details come from LlamaIndex's public documentation and may change over time.
