# MCP tools

anymd exposes three tools.

| Tool | Use it to |
|---|---|
| [`read`](#read) | Turn a file, URL, or folder into Markdown |
| [`search`](#search) | Find text across files, folders, and URLs |
| [`inspect`](#inspect) | Go deeper on a PDF: render pages, crop regions, OCR, JSON structure, compare |

## read

Any file, URL, or directory → Markdown.

| Argument | Type | Default | Description |
|---|---|---|---|
| `source` | string | required | File path, `http(s)` URL, or directory. A directory returns the list of readable files. |
| `pages` | string | all | Pages (PDF), slides, sheets, or chapters, e.g. `"1-5,8"` |
| `max_tokens` | number ≥ 500 | `20000` | Token budget. Longer documents stop at a page/slide/chapter boundary and end with a cursor. |
| `cursor` | string | – | Continue a previous read with the cursor from its last line |
| `ocr` | boolean | automatic | OCR images and image-only PDF pages with a local `tesseract`. Automatic when tesseract is installed; `false` disables. |
| `transcript` | boolean | `false` | Transcribe audio/video with a local whisper.cpp |
| `download_whisper_model` | boolean | `false` | Implies `transcript`; downloads the ggml model (base.en, 148 MB, SHA-256 verified) into the anymd cache when none is installed |

```json
{ "source": "papers/attention.pdf" }
```

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

### Cursor and pagination

When a document is larger than `max_tokens`, `read` stops at a unit boundary (a page, slide, sheet, or chapter) and its last line names the cursor to continue from. Pass it back unchanged:

```json
{ "source": "papers/attention.pdf", "cursor": "10" }
```

A cursor is `"<page>"` or, when a single page was larger than the whole budget and had to be split, `"<page>:<offset>"`. You can also skip the cursor and ask for exactly what you need with `pages`, or raise `max_tokens`.

### Folders and URLs

`read` on a directory lists the readable files in it, so the agent can pick one or `search` them all. `read` on a web URL returns the main article only; see [Formats](./formats#html-and-urls).

## search

Find text across files, directories, and URLs, one line per hit.

| Argument | Type | Default | Description |
|---|---|---|---|
| `query` | string | required | Text to find |
| `sources` | string[] | current directory | Files, directories (searched recursively, `.gitignore` respected), or URLs. Up to 256. |
| `mode` | string | `auto` | `auto`: exact phrase, falling back to ranked passages when nothing matches. `literal`: exact phrase only. `ranked`: BM25 over the query words. |
| `glob` | string | – | Only files matching this glob inside directories, e.g. `"*.pdf"` or `"reports/**"` |
| `case_sensitive` | boolean | `false` | Match case |
| `whole_word` | boolean | `false` | Match whole words |
| `max_results` | number 1–500 | `20` | Maximum hits |
| `context_chars` | number 0–1000 | `80` | Snippet context on each side |

```json
{ "query": "masked language model", "sources": ["papers/"] }
```

```markdown
5 matches for "masked language model" (2 files, 31 sections searched)

### papers/bert.pdf (5)
- p.1: …by using a “**masked language model**” (MLM) pre-training objective, inspired by the Cloze task…
- p.2: …In addition to the **masked language model**, we also use a “next sentence prediction” task…
```

In `auto` mode, a question like "how does bidirectional pretraining work" that matches nothing exactly still finds the right page through BM25-ranked passages.

## inspect

Go deeper on a PDF. `inspect` returns JSON, for the cases where an agent needs geometry, images, or a diff rather than prose.

| `operation` | What it does |
|---|---|
| `inspect` | Page count, metadata, and per-page facts |
| `render_page` | Render pages to PNG images |
| `extract_regions` | Crop regions (bounding boxes) out of rendered pages |
| `ocr_pages` | OCR pages with the local OCR provider |
| `analyze_regions` | Send regions to a configured vision/region provider |
| `structure` | Structured JSON read: document map, elements, geometry, tables, and reports |
| `compare` | Page-level text diff between two local PDFs (`sources[0]` = before, `sources[1]` = after) |

| Argument | Type | Description |
|---|---|---|
| `operation` | string | One of the operations above (required) |
| `sources` | object[] | `{ path \| url, pages?, regions? }` per PDF. A region is `{ page, bounding_box: { left, bottom, right, top }, padding? }`. |
| `profile` | string | `structure` only: `fast` (default), `quality`, or `research` (adds safety, trust, and accessibility reports) |
| `scale` | number 0.25–4 | Render scale for `render_page` / `extract_regions` |
| `max_pages` | number 1–20 | Page cap for rendering and OCR |
| `max_regions` | number 1–100 | Region cap |
| `max_pixels_per_page` | number | Pixel cap per rendered page |
| `include_image` | boolean | Include the rendered PNG in the response |
| `languages` | string[] | OCR languages, e.g. `["eng", "chi_tra"]` |
| `timeout_ms` | number | Time limit for external tools (1,000–300,000) |
| `max_output_chars` | number | Output cap (1,000–1,000,000) |

Render page 3 at 2×:

```json
{ "operation": "render_page", "sources": [{ "path": "report.pdf", "pages": [3] }], "scale": 2 }
```

Get the structured JSON (elements with bounding boxes, tables, outline):

```json
{ "operation": "structure", "sources": [{ "path": "report.pdf" }], "profile": "quality" }
```

Diff two versions:

```json
{ "operation": "compare", "sources": [{ "path": "contract-v1.pdf" }, { "path": "contract-v2.pdf" }] }
```
