# Product proof

This is the acquisition-facing evidence board for PDF Reader MCP.

> **Give your AI agent eyes for PDFs.**

One local MCP server returns structured text, tables, OCR paths, visual evidence, and page-level citations.

![Before vs after](/before-after-evidence.svg)

## Install now

```bash
npm install -g @sylphx/citra
claude mcp add pdf-reader -- npx @sylphx/citra
```

## Three flagship workflows

### 1. Financial report / tables

```json
{
  "sources": [{ "path": "/absolute/path/to/10-k.pdf" }],
  "include_tables": true,
  "include_markdown": true,
  "pages": [14]
}
```

**Outcome:** table cells with page + geometry — not a prose guess about revenue.

### 2. Research paper / citations

```json
{
  "sources": [{ "path": "/absolute/path/to/paper.pdf" }],
  "include_full_text": true,
  "include_document_map": true
}
```

**Outcome:** reading-order structure and page-linked quotes agents can cite.

### 3. Scanned document / OCR

```json
{
  "sources": [{ "path": "/absolute/path/to/scan.pdf" }],
  "include_ocr_text_layer": true,
  "include_tables": true
}
```

**Outcome:** OCR kept separate from selectable text, with page evidence for verification.

More copy-ready examples: [examples/demo](https://github.com/SylphxAI/citra/tree/main/examples/demo).

## Why agents need more than text

| Without evidence | With PDF Reader MCP |
| --- | --- |
| “Revenue was about $12M” | Page 14 · Table 3 · cell (4,2) = `$12.4M` |
| Tables become paragraphs | Rows, columns, cells, geometry |
| Scanned PDFs become noise | OCR path with page-linked evidence |
| Hidden text ignored | Trust signals when requested |

## Install footprint (measured)

Clean install on **linux-x64** (Node 24):

| Metric | TS `3.0.14` | Sole-Rust `4.1.x` |
| --- | ---: | ---: |
| Full `node_modules` | ~82.3 MiB | **~24.4 MiB (~3.4× smaller)** |
| Files | 4,101 | **20 (~205× fewer)** |
| Production deps | PDF.js + MCP SDK + … | `{}` + one native package |

Native binary is multi-megabyte because it **is** the engine — and still yields a cleaner install than a JS dependency tree.

## Performance (bounded, authorized)

Controlled **same-host linux-x64** dual-mode A/B vs `@sylphx/pdf-reader-mcp@3.0.14` using **registry-installed 4.1.0/4.1.1** natives:

| Mode | Measures | Result |
| --- | --- | --- |
| `persistent_warm` | long-lived server, repeated identical local `read_pdf` after warm-up | **≥ ~10×** on all 8 required fixture classes |
| `startup_inclusive` | spawn + initialize + one task | large advantage |

`persistent_warm` includes a process-local cache for identical local path+options. First request still pays full parse cost.

**Not** a multi-host / RSS / OCR-provider guarantee.

Details:

- [4.1.0 same-host performance report](https://github.com/SylphxAI/citra/blob/main/docs/specs/performance/4.1.0-same-host-performance-report.md)
- [4.1.0 claims policy](https://github.com/SylphxAI/citra/blob/main/docs/specs/performance/4.1.0-performance-claims-policy.md)

## Engine

Version 4+ uses a **native Rust engine** via a thin Node launcher.

- Five platforms
- Fail closed if native binary missing
- No TypeScript PDF runtime shipped in production

## What stays secondary

Migration ledgers, residual parity families, CI SHA archaeology → [migration notes](/migration).
