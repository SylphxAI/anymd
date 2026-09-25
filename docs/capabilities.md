# Capabilities — anymd

## Surfaces

| Surface | Identity |
| --- | --- |
| MCP | `io.github.SylphxAI/anymd` over stdio, `npx -y @sylphx/anymd` |
| CLI | `anymd` |
| SDK | `@sylphx/anymd/sdk` |

## Owned capabilities

| Capability | Tool | Evidence |
| --- | --- | --- |
| Structured PDF read | `read_pdf` | page, table/cell indices, bbox, source hash, warnings, gaps |
| Literal search | `search_pdf` | page and bounding-box locators before a deep read |
| Document comparison | `pdf_compare` | page- and term-level comparison of two local PDFs |
| Focused verification | `pdf_evidence` (`inspect`, `render_page`, `extract_regions`, `ocr_pages`, `analyze_regions`) | render, crop, OCR and region evidence with provenance |

## Evidence contract

Every result carries `route`, locators, source identity, warnings and known gaps. See [EVIDENCE_CONTRACT.md](./EVIDENCE_CONTRACT.md).

## Not owned

Cloud OCR as the default path, generative summaries as evidence, filesystem mutation, and deliberation.
