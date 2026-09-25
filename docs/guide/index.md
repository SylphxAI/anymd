# anymd

**Give your AI agent eyes for PDFs — with proof.**

anymd is the local-first PDF evidence tool for AI agents. One `read_pdf` call
returns markdown, tables with cells and geometry, and **page-level citations
your agent can defend**. OCR stays off until you ask for it.

::: tip Ten minutes to a citable claim
Start with the [Quickstart](/guide/getting-started). It walks one document from
install to a claim a human can check.
:::

## Who this is for

You are building agents that read **contracts, filings, research papers,
invoices, or scanned records** — and you have been burned by an agent that
confidently quoted a number it invented. anymd exists for that failure.

## What makes it different

A text extractor answers *"what characters are on this page?"* anymd answers
*"what can my agent safely assert, and where does the proof live?"*

Every claim in a response can carry:

- **page** — where it is in the document
- **bounding box** — where on the page, in PDF coordinates
- **table / cell indices** — for numbers that came from a grid
- **provenance** — which engine and which layer (selectable text vs OCR) produced it
- **quality signals** — when the evidence is weak (sparse cells, merged cells, incomplete geometry)
- **gaps** — what it could **not** prove, named instead of guessed

That is [the evidence contract](/EVIDENCE_CONTRACT), and it is the whole point of
the product.

## The four tools

| Tool | Job |
| --- | --- |
| [`read_pdf`](/api/) | Fast read — markdown, tables, geometry, and citation-ready chunks. No OCR. |
| [`search_pdf`](/api/) | Cheap literal retrieval with page and bounding-box locators |
| [`pdf_compare`](/api/) | Compare two PDFs at page and term level |
| [`pdf_evidence`](/api/) | Focused verification: `inspect`, `render_page`, `extract_regions`, `ocr_pages`, `analyze_regions` |

Few, powerful, obvious. Advanced work lives behind one `op` enum instead of
accumulating near-duplicate tool names — see [the tool surface](/TOOL_SURFACE).

## Local-first

Reading a local PDF needs **no** network, **no** API key, and uploads **no**
document. OCR and region analysis are opt-in providers you choose. See
[the local-first frontier](/LOCAL_FIRST_FRONTIER).

## Native and fail-closed

The PDF engine is native Rust, selected per platform at install. The launcher
ships zero production JS dependencies. If the matching native binary is
missing, the process refuses to start — there is no silent fallback to a
different engine.

## Install in 30 seconds

```bash
npx -y @sylphx/anymd
```

Full per-client setup: [Installation](/guide/installation)

## Where to go

| If you want… | Go to |
| --- | --- |
| to use it right now | [Quickstart](/guide/getting-started) |
| every option and result field | [API reference](/api/) |
| to know what "proof" means here | [The evidence contract](/EVIDENCE_CONTRACT) |
| the speed claims and their bounds | [Performance](/performance/) |
| why not the alternatives | [Comparison](/comparison/) |
| to report a vulnerability | [Security reporting](/security/maintainer-process) |
