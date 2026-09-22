# Citra — competitive positioning

## Job

PDF evidence for agents

## Wedge

Local-first native PDF intelligence with page/cell/bbox evidence — not a cloud OCR wrapper.

## Local-first

Default path uses local native runtime; no cloud API key required for core read.

## Peer anchors (learn; do not clone)

| Peer | Gap we exploit |
| --- | --- |
| Cloud/API OCR MCPs (e.g. Mistral OCR MCP) | Paid remote OCR; weak citeable structure/tables locally |
| Paperless-style archive MCP | Archive/search system, not agent citeable PDF structure toolkit |
| Generic filesystem MCP + raw PDF text | No structure, tables, visual evidence, or page locators |

## Non-goals

- Becoming a cloud SaaS wrapper as the default path
- Multi-product monorepo for star aggregation
- Generative summaries as the sole evidence authority

## Zero-config CTA

```bash
npx -y @sylphx/citra
```

Bare invoke starts brand-sole MCP on stdio. Live: the latest `@sylphx/citra` on npm.
