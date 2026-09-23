# Comparison

The job: **PDF evidence for agents.** Not a cloud OCR wrapper, not an archive
system, not "whatever text we could scrape."

| Approach | What your agent gets | The gap |
| --- | --- | --- |
| **Cloud / API OCR MCPs** | Text from a paid remote OCR call | Documents leave the machine; structure and tables come back weak; per-call cost; no page-level cell geometry |
| **Archive / document-management MCPs** | Search over a repository you built | Archive search, not an agent toolkit for citeable structure — you still lack tables, crops, and locators |
| **Filesystem MCP + raw PDF text** | A wall of characters | Page numbers invented, tables flattened, scans become noise, regions impossible to cite |
| **Ask a vision model** | A fluent summary | Unverifiable. The model reads an image; it does not return a page, a cell, or a bounding box you can check |
| **PDF.js / a JS PDF library in-process** | Text you parse yourself | You own the parser, the OCR, the table model, the failure modes — and the install footprint |
| **Citra** | Fast local read: markdown, tables with cell geometry, and page citations. OCR and crops only when you ask | — |

## The distinction that matters

A text extractor answers *"what characters are on this page?"* Citra answers
*"what can my agent safely assert, and where does the proof live?"*

That is why the response carries `page`, `bounding_box`, `provenance`, `quality`
signals, and `gaps` — not just `full_text`. See
[the evidence contract](/EVIDENCE_CONTRACT).

## Named peers

These are the tools agents actually get compared with. Citra does not claim a
speed win over them; that has not been measured here. The difference is the
default contract.

| Peer | Best at | Citra's default |
| --- | --- | --- |
| [Docling](https://github.com/docling-project/docling) | Model-backed layout conversion | No model on the fast path. Page and cell locators in the MCP response. |
| [Marker](https://github.com/datalab-to/marker) | PDF to markdown with a layout model | Fast preset is embedded text and tables. `profile: quality` adds structure without OCR. |
| [PyMuPDF4LLM](https://pymupdf.readthedocs.io/en/latest/pymupdf4llm/) | Local markdown for RAG pipelines | An MCP server, plus cell geometry and citation chunks. |
| [LlamaParse](https://developers.llamaindex.ai/python/cloud/llamaparse/) | Hosted parsing quality | The PDF stays local. No API key for `read_pdf`. |
| [`@modelcontextprotocol/server-pdf`](https://github.com/modelcontextprotocol/servers) | Minimal MCP text extraction | Tables, geometry, and an explicit OCR tool instead of a text dump. |


## Local-first, for real

The default path never needs a network. No document upload, no API key, no
per-call charge for reading a local PDF. Providers are **opt-in** for OCR and
region analysis only — and even then the evidence stays linked to the local
document. See [the local-first frontier](/LOCAL_FIRST_FRONTIER).

## Three tools, one surface

| Tool | Job |
| --- | --- |
| `read_pdf` | fast read: markdown, tables, geometry, citations |
| `search_pdf` | cheap locate with locators |
| `pdf_evidence` | focused verify: inspect / render / crop / OCR / regions |

Advanced work lives behind one `op` enum instead of accumulating near-duplicate
tool names. See [the tool surface](/TOOL_SURFACE).

## Honest about limits

- Performance numbers are **method-bounded** (same host, named mode, named task
  family) — see [Performance](/performance/).
- OCR and region analysis need an opt-in provider. Core reading does not.
- Generative summaries are not evidence. Citra returns facts with locators; what
  your agent concludes is its own responsibility.

## Next

- [The evidence contract](/EVIDENCE_CONTRACT)
- [Product proof](/guide/product-proof)
- [Performance](/performance/)
