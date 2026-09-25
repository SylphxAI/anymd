# anymd — PDF evidence for agents

## Install

```bash
npm i -g @sylphx/anymd
# or
npx @sylphx/anymd
```

## Tools

| Tool | Job |
| --- | --- |
| `read_pdf` | Agent Document Twin (structure, tables, citations) |
| `search_pdf` | Literal search with page/bbox evidence |
| `pdf_evidence` | inspect / render / crop / OCR follow-ups |

## SDK

```ts
import { Anymd } from '@sylphx/anymd/sdk'
const anymd = Anymd.create()
const result = await anymd.read({ path: '/abs/doc.pdf' })
```

## Rules

- Local-first; no API key required for default path
- Missing native binary → fail closed
- Evidence on results (envelope v1); no `evidence_first` tool
- Do not install Prism for PDF routing
