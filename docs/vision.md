# anymd vision

## Goal

Any file becomes clean Markdown for an AI agent in one call, on the user's machine, without an API key. Agents get accurate text, reading order and tables with page, region and source provenance, so they can cite and verify what they read.

## Who it is for

Agent and MCP users who need documents in context (PDF, Office, EPUB, HTML, images, audio and video), and developers who want a fast local converter on the command line.

## Boundaries

- Local-first: documents are not uploaded and no remote provider is called unless the caller selects one.
- One Rust binary (`crates/anymd`) is the MCP server and the CLI; the npm package is a launcher for it. The Rust server is the only authority for the public MCP tool schemas.
- OCR, vision and region analysis run behind typed adapters that fail closed.
- Out of scope: hosted accounts, billing, storage, tenancy, customer data retention, and product-specific model routing or provider secrets. A hosted document service would be a separate product.
- Package names: `@sylphx/anymd` (bin `anymd`), platform packages `@sylphx/anymd-<platform>`, and the aliases `@sylphx/citra` and `@sylphx/pdf-reader-mcp` (formerly the product names) at the same version.

## Target metrics

Measured by AgentDocBench ([bench/README.md](../bench/README.md), leaderboard in [guide/benchmarks.md](guide/benchmarks.md)):

- Highest overall score among the tools compared, with 100% of documents converted.
- Table cell F1 and reading order at or above the best compared tool.
- Total conversion time at least an order of magnitude below the model-based tools.
- Output tokens per document below the compared tools at equal accuracy.

## Where things live

- Public surfaces: `README.md`, `docs/`, `crates/anymd`, `packages/anymd`.
- CI: `.github/workflows/ci.yml`; release: `.github/workflows/release.yml` ([PUBLISH.md](PUBLISH.md)).
- Brand: `brand/` ([README](../brand/README.md)).
