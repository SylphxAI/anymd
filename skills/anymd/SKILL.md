---
name: anymd
description: Read any file as Markdown with the anymd MCP server - PDF, Word, PowerPoint, Excel, CSV, EPUB, HTML and web pages, images (OCR), audio and video. Use it when a task needs the contents of a document, a folder of documents, or a URL, or needs to find text across them.
---

# anymd

anymd turns files, folders and URLs into Markdown, locally, with no API key.

## Tools

| Tool | Use it to | Key arguments |
| --- | --- | --- |
| `read` | Turn a file, URL, or folder into Markdown | `source`, `pages` (`"1-5,8"`), `max_tokens` (default 20000), `cursor`, `ocr`, `transcript` |
| `search` | Find text across files, folders, and URLs | `query`, `sources`, `mode` (`auto`, `literal`, `ranked`), `glob`, `max_results` |
| `inspect` | Go deeper on a PDF | `operation`: `render_page`, `extract_regions`, `ocr_pages`, `structure`, `compare`, `inspect` |

## How to use it

- Read long documents in parts: pass `pages`, or continue with the `cursor` that `read` returns when it stops at the token budget.
- Cite pages from the `<!-- page N -->` anchors in `read` output.
- Use `search` before reading a whole folder. With no exact match it returns BM25-ranked passages.
- Use `inspect` only for PDF layout, page images, regions or OCR.

## CLI

```bash
anymd report.pdf > report.md
anymd search "indemnification" contracts/ --glob '*.pdf'
anymd doctor
```
