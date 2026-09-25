# Migration and recovery notes

This page is **secondary engineering history**. Product install lives on the [Installation](/guide/installation) guide and repository README.

## 6.0 — fast is the default

`read_pdf` with only `sources` now uses the **fast** preset. It no longer
returns safety findings, a trust report, or an accessibility report unless you
ask. `profile: "quality"` adds structure and still does not run OCR.
`profile: "research"` adds the audits. `auto: true` keeps the previous
balanced behavior. `auto_detail` wins over `profile`. A `pages` list filters
the read and does not turn the preset off. OCR remains `include_ocr_text_layer`
or `pdf_evidence`.

Callers that depended on a sources-only trust report should send
`"profile": "research"` or `"auto": true`.

## Current production

- Current production identity: `@sylphx/anymd` (version: see npm)
- Engine: native Rust via thin Node launcher
- No TypeScript PDF runtime in the production package
- No `./typescript` export
- Missing native package fails closed

## Install

Use the brand-sole anymd package:

```bash
npx -y @sylphx/anymd
```

The retired `@sylphx/pdf-reader-mcp` package ID is historical evidence and a
comparison pin, not a current install or publish path.

## Historical TypeScript baseline

Immutable external baseline for comparison/recovery only:

```bash
npm install -g @sylphx/pdf-reader-mcp@3.0.14
```

## Transitional history

- `3.2.2`: Rust-default with bundled TypeScript rollback (not Sole Rust)
- `4.0.0`: Sole-Rust runtime cutover that still declared TS/PDF.js production dependencies
- `4.0.1`: empty production dependency graph; Linux x64 native packaging defect
- `4.0.2`: Sole-Rust dependency-closure with installable five-platform natives
- `4.1.0`: warm read cache + strip/LTO natives + product performance evidence
- `4.1.1`: npm README / claims sync of the authorized 4.1 narrative
- `5.0.0`: hard cut to the brand-sole `@sylphx/citra` package, `citra` command,
  Citra native package family, and `io.github.SylphxAI/citra` MCP identity
- `7.0.0`: rename to **anymd** — `@sylphx/anymd` package, `anymd` command,
  `@sylphx/anymd-<platform>` native packages, and `io.github.SylphxAI/anymd` MCP
  identity. `@sylphx/citra` and `@sylphx/pdf-reader-mcp` return as thin aliases
  published at the same version, so old install commands keep working.

## Performance admission status

Registry-bound dual-mode evidence for the 4.1 lineage remains historical and
method-bounded. It does not by itself prove current anymd target coverage; see
the current host-runtime and registry-install proof workflows.
