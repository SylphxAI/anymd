# anymd — PDF evidence for agents

## Install

```bash
npx -y @sylphx/anymd setup   # add it to the MCP clients on this machine
npm i -g @sylphx/anymd       # or install the CLI
```

## Tools

| Tool | Job |
| --- | --- |
| `read_pdf` | Agent Document Twin (structure, tables, citations) |
| `search_pdf` | Literal search with page/bbox evidence |
| `pdf_evidence` | inspect / render / crop / OCR follow-ups |

## CLI

```bash
anymd /abs/doc.pdf > doc.md
anymd search "indemnification" contracts/
```

## Rules

- Local-first; no API key required for default path
- Missing native binary → fail closed
- Evidence on results (envelope v1); no `evidence_first` tool
- Do not install Prism for PDF routing
