# Tool surface — anymd

Policy: **few, powerful, obvious** tools.

| Tool | Role |
| --- | --- |
| `read_pdf` | Primary structured PDF read (text, tables, map, citations) |
| `search_pdf` | Literal retrieval with page/bbox locators |
| `pdf_compare` | Page- and term-level comparison of two local PDFs |
| `pdf_evidence` | Follow-up ops: inspect / render / crop / OCR / regions (`op` enum) |

## Surfaces

| Surface | Role |
| --- | --- |
| MCP | Agent tools over stdio |
| CLI | `anymd` |
| SDK | `@sylphx/anymd/sdk` |

## Rules

1. No near-duplicate vanity tools.
2. Advanced ops live inside `pdf_evidence`, not new tool names.
3. Fail closed on unsafe input / missing native.
4. Composition with companion tools is through public MCP and SDK contracts only.
