# anymd — competitive positioning

## Job

PDF evidence for agents: a local read with page and cell locators, cheap unless
you ask for more.

## Wedge

Local-first native PDF reading with page, cell, and bounding-box evidence.
Not a cloud OCR wrapper, and not a model that runs on every call.

## What the default is

`read_pdf` with only `sources` is the **fast** preset: markdown, tables, chunks,
a document map, geometry, and layout. OCR, rendering, and trust audits are
separate requests.

## Peer anchors

Learn from them. Do not pretend to be them.

| Peer | What they are strong at | What anymd keeps different |
| --- | --- | --- |
| [Docling](https://github.com/docling-project/docling) | Layout models and document conversion | No model download on the default call. An MCP tool with explicit page evidence. |
| [Marker](https://github.com/datalab-to/marker) | High-quality PDF to markdown | The fast path does not load a layout model. Deeper structure is `profile: quality`. |
| [PyMuPDF4LLM](https://pymupdf.readthedocs.io/en/latest/pymupdf4llm/) | Fast local markdown for RAG | anymd is an MCP server and returns cell geometry and citations, not only markdown. |
| [LlamaParse](https://developers.llamaindex.ai/python/cloud/llamaparse/) | Strong cloud parsing | Documents stay on the machine. No API key for the core read. |
| [`@modelcontextprotocol/server-pdf`](https://github.com/modelcontextprotocol/servers) | A small MCP text extract | Tables, geometry, chunks, and an explicit way to ask for OCR. |

## Non-goals

- A cloud API as the default path
- Silent OCR or a trust audit on every read
- Generative summaries as the evidence

## Install

```bash
npx -y @sylphx/anymd
```
