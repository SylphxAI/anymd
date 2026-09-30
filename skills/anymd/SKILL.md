---
name: anymd
description: Read any file as Markdown with the anymd MCP server - PDF, Word, PowerPoint, Excel, CSV, EPUB, HTML and web pages, images (OCR), audio and video. Use it when a task needs the contents of a document, a folder of documents, or a URL, or needs to find text across them.
---

# anymd

anymd turns files, folders and URLs into Markdown, locally, with no API key.

## Tools

| Tool | Use it to | Key arguments |
| --- | --- | --- |
| `outline` | Navigate a document heading tree without a model | `source`, `format` (`json` default, or `tree`); returns stable ids, title paths, unit and Markdown byte ranges |
| `read` | Turn a file, URL, or folder into Markdown | `source`, `node` (id from outline), `pages` (`"1-5,8"`), `max_tokens` (default 20000), `cursor`, `ocr`, `images` (`refs` default: figures inside PDF/DOCX/PPTX/EPUB are saved as files you can open, marked `![caption](path)`; `none` to skip), `revisions` (Word tracked changes: `markup` default, as CriticMarkup with author and date; `accept` or `reject` for the clean text after Accept All or Reject All), `transcript` |
| `search` | Find text across files, folders, and URLs | `query`, `sources`, `mode` (`auto`, `literal`, `ranked`), `glob`, `max_results` |
| `inspect` | Go deeper on a PDF | `operation`: `render_page`, `extract_regions`, `ocr_pages`, `structure`, `compare`, `inspect` |

## How to use it

- Navigate a long document: call `outline`, pick a node by title path, then `read` with the same source and `node`. Search hits also carry a node id and title path. Repeat `node` with `cursor` to continue inside that section. Outline and node reads default to no OCR/image export and Word revision markup; if you change `ocr`, `images`, or `revisions`, pass them to both tools. Search uses the default outline options. IDs are stable for the unchanged source and extraction options; get a fresh outline after edits.
- Read long documents in parts: pass `pages`, or continue with the `cursor` that `read` returns when it stops at the token budget.
- Cite pages from the `<!-- page N -->` anchors in `read` output.
- Use `search` before reading a whole folder. With no exact match it returns BM25-ranked passages.
- Use `inspect` only for PDF layout, page images, regions or OCR.

## CLI

```bash
anymd outline report.pdf --format json
anymd report.pdf --node n1.2
anymd report.pdf > report.md
anymd search "indemnification" contracts/ --glob '*.pdf'
anymd doctor
```
