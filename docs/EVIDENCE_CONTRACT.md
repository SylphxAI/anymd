# Evidence contract — Citra

**Evidence First** means results carry citeable structure. There is **no** MCP tool named `evidence_first`.

Family wire law: `SylphxAI/skills` `schemas/product-evidence-envelope.schema.json` (envelope_version `"1"`).

## Locators and honesty (Citra)

- page number
- table/cell indices when extracted
- bbox / region when visual/OCR
- source path + document hash when available
- warnings for OCR confidence / partial parse
- gaps for missing text layer, failed pages, denied ops

## Always include when applicable

- **route**: local engine path (rust-core / adapter)
- **warnings** / **gaps** arrays (may be empty)
- raw facts over generative rewrite as authority

## Non-goals

- Cloud model confirmation of local facts
- Marketing “Evidence First” without locators on the wire
