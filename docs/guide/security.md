---
description: "How anymd keeps documents local: URL fetch guards, directory limits and sandboxed external tools."
---

# Security

## Local-first

anymd runs on your machine. Documents never leave it unless you pass a URL, and even then only that URL is fetched. No account or API key is involved.

## URL fetches

When `read` or `search` is given a URL:

- Private, loopback, and link-local addresses are blocked, so a prompt cannot point anymd at your router, cloud metadata endpoint, or local services.
- Every redirect hop is checked again and pinned to the address it resolved to, so DNS rebinding cannot swap in a private address between the check and the connection.
- Downloads are capped at 256 MB.

## Confining file access

By default the server can read any file your user can. To limit it to specific folders, pass `--allow-dir` (repeatable) or set `MCP_PDF_ALLOWED_DIRS`:

```bash
claude mcp add anymd -- npx -y @sylphx/anymd --allow-dir=$HOME/docs --allow-dir=$HOME/papers
```

```json
{
  "command": "npx",
  "args": ["-y", "@sylphx/anymd"],
  "env": { "MCP_PDF_ALLOWED_DIRS": "/home/me/docs" }
}
```

Paths outside the allowed directories, including ones reached through symlinks, are refused.

## HTTP transport

stdio is the default. `MCP_TRANSPORT=http` serves MCP on `MCP_HTTP_HOST:MCP_HTTP_PORT` (default `127.0.0.1:8080`). Set `MCP_API_KEY` and every `/mcp` request must send a matching `X-API-Key` header, or it gets `401`; `/mcp/health` stays open. The server refuses a non-loopback bind without a key. `MCP_CORS_ORIGIN` sets an explicit CORS origin.

## External tools

`tesseract`, `ffprobe`, `ffmpeg`, and the optional ForcedAligner helper are optional. When anymd uses them, it runs them directly, without a shell, with a timeout and an output cap. Nothing from a document is ever interpreted as a command.

## Parsing untrusted files

Every format is parsed in Rust. Resource ceilings are separate from the response budget: `read` still defaults to 20,000 tokens and returns a continuation cursor for longer documents. A response budget does not refuse an otherwise valid book or deck.

### Conversion ceilings

Non-PDF conversion has a fixed **256 MiB (268,435,456 bytes) of Markdown** ceiling, summed across sections. EPUB, DOCX and PPTX also have a fixed **512 MiB (536,870,912 bytes) of cumulative inflated input** ceiling. Every container read counts again, including repeated chapter or slide references, image discovery, images and optional metadata. Conversion stops with the typed `ConvertError::LimitExceeded`, for example `conversion output exceeds 268435456 bytes`; it does not return a partial document. No caller option raises these ceilings, including CLI conversion without a response budget.

| Format | Enforcement |
| --- | --- |
| EPUB | Chapters are read and rendered one at a time; cumulative input and Markdown are checked before retaining another chapter. Image-discovery reads use the same counter. Existing limits of 10,000 ZIP entries, 200 MiB declared total inflation and 50 MiB per entry remain. |
| DOCX / PPTX | Shared ZIP accounting counts repeated part and media reads. Block writers and table rendering stop at the Markdown ceiling; PPTX sums output across slides. Existing limits of 10,000 ZIP entries and 200 MiB per entry remain. |
| XLSX | A preflight inflates and measures actual ZIP entries under the input ceiling before calamine opens them. Initialization bytes are reserved and each worksheet target is charged again before parsing, including repeated references. Each sheet's rendered table is checked, and Markdown is summed across sheets. |
| XLS / XLSB / ODS | Markdown is summed across sheets. ZIP-based variants have the same measured preflight; their parser's internal reads and allocations are not instrumented. |
| HTML / CSV / TSV | Block and table rendering check the Markdown ceiling, including ragged-table padding. CSV/TSV and spreadsheet tables still keep at most 2,000 data rows plus the header. |
| Text / subtitles / images / audio / video | The non-PDF result is checked against the Markdown ceiling. This final check is not an early-stop guarantee for these converters. External helpers also have their own timeout and captured-output caps. |
| PDF | Separate page-oriented limits apply. Raster rendering defaults to 20 selected pages, 16,000,000 pixels per page and 64 MiB encoded output per page. These are not a document-wide Markdown ceiling. |

Repeated spine items and slide references are preserved in reading order, not deduplicated. Distinct content is never silently dropped to satisfy a ceiling. Output below the ceilings retains the existing formatting, response truncation, totals and paging behavior.

These are byte-accounting ceilings, not a process-memory limit: input buffers, XML trees, spreadsheet ranges and parser scratch allocations can use additional memory. For fully untrusted input, use an operating-system memory and CPU limit as well.

### Sizing basis

CI measured all 74 checked-in EPUB, PPTX, DOCX and XLSX documents in `test/fixtures`, `bench/files` and the format fixtures on 2026-11-07, before the accounting change. The largest rendered / unique inflated sizes in bytes were:

| Format | Markdown | Inflated ZIP entries | Largest file |
| --- | ---: | ---: | --- |
| DOCX | 16,923 | 397,786 | `bench/files/docx-hhs-market-research.docx` |
| EPUB | 238 | 3,725 | `bench/files/epub-markitdown.epub` |
| PPTX | 169 | 6,059 | `test/fixtures/slides.pptx` |
| XLSX | 1,261 | 47,082 | `bench/files/sheet-ers-crop-insurance.xlsx` |

This corpus is small; it does not establish a maximum size for real-world books or workbooks. The ceilings provide more than 15,000 times the measured Markdown maximum and more than 1,300 times the measured unique inflation maximum. Additional generated EPUB, PPTX and XLSX controls keep more than 32 MiB of content, leaving roughly eight times that output headroom. CI also exercises a refusal through the public converter at the real production ceiling and reports elapsed time and peak resident memory.

## Reporting a vulnerability

See [SECURITY.md](https://github.com/SylphxAI/anymd/blob/main/SECURITY.md) for how to report a vulnerability privately.
