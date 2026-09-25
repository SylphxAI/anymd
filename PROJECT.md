# anymd

Any file → clean Markdown for AI agents.

PDF Reader MCP is a local-first public Model Context Protocol package for PDF
and document intelligence. It gives agents typed tools for PDF inspection,
search, rendering, region crops, OCR routing, extraction, document maps, trust
signals, accessibility reports, provenance, benchmarks, and package release
evidence without becoming a hosted Sylphx Platform BaaS service.

## Identity

- Brand: **anymd** — "Any file → clean Markdown for AI agents"
- Canonical npm: `@sylphx/anymd` (bin `anymd`; MCP registry `io.github.SylphxAI/anymd`)
- Native optional packages: `@sylphx/anymd-<platform>` (binary `anymd`) in
  `packages/npm/<platform>`
- Compatibility aliases, published at the same version as `@sylphx/anymd`:
  `@sylphx/citra` (bin `citra`) and `@sylphx/pdf-reader-mcp` (bin `pdf-reader-mcp`)
  in `packages/aliases/`. `scripts/set-version.ts` keeps every manifest at one version.
- Repository: `SylphxAI/anymd`, formerly `SylphxAI/citra` and
  `SylphxAI/pdf-reader-mcp` (the old slugs redirect here).

## Lifecycle

- Lifecycle: `production`
- Layer: `tooling`
- Project facts and human projection: this file (`PROJECT.md`)
- Agent local notes: `AGENTS.md`
- Retired lineage (do not load as instruction or live state): Doctrine,
  Mission Control, GroundAtlas package dogfood / adapters

## Goals

- Own the public MCP tool schemas, sole-Rust production runtime, provenance
  model, provider-neutral optional OCR/vision interfaces, benchmarks, docs, and
  release evidence.
- Keep document processing local-first by default with explicit local-vs-remote
  provider behavior.
- Preserve source/page/region provenance so downstream agents can cite and
  verify evidence.

## Non-Goals

- Do not become Sylphx Platform Auth, Billing, Storage, Durable Work, Gateway,
  hosted multi-tenant execution, or customer data retention.
- Do not own Spiron, Cubeage, or customer-specific product semantics.
- Do not store direct provider secrets or product-specific model routing inside
  this package.

## Boundaries

PDF Reader MCP owns the local/open-source document-intelligence package and its
public MCP contract. The Rust crates and the native binary are the only
implementation; the npm package is mcp-kit's launcher, which runs that binary.

It does not own hosted customer accounts, billing, storage, tenant policy,
Gateway routing, product audit, or durable state created after a tool is used.
Hosted document intelligence must be a separate service with its own spec
and commercial controls.

## Public Surfaces

- MCP package and CLI: `packages/anymd` → `bin/anymd.js` → the platform binary
- Rust core / server: `crates/anymd-core`, `crates/anymd`
- Public docs: `README.md`, `docs/`
- Tool/spec docs: `docs/specs/`
- CI: `.github/workflows/ci.yml`
- Release: `.github/workflows/release.yml`, calling the shared mcp-kit release
  workflow (see `docs/PUBLISH.md`)

## Delivery

Terminal delivery is **npm package release** (main package + platform optional
native packages) with registry readback — not a hosted app deploy.

Pull requests run `CI` on GitHub-hosted runners; `Validate Code Quality`,
`security:secrets` and `Plain language` are the required checks. Merging a
version bump to `main` publishes through `release.yml`: 5 native builds, npm
with trusted publishing, an `npx` smoke test, the GitHub release and the MCP
Registry entry.

A past Control Plane decision retired in-repository GroundAtlas package dogfood.
Doctrine adapters and Mission Control are retired historical lineage and must
not be restored as machine truth. Release and adoption claims must be verified
against source, CI, published registries, and clean consumer installs.

Docs-only boundary changes do not alter runtime behavior, provider dispatch,
credentials, package output, npm release, or customer data handling. MCP schema,
handler, parser, provider, benchmark, or package changes require focused tests,
docs, build/coverage evidence, release intent, and npm readback after publish.

## Commercial Direction

This repo is a public utility package and can support commercial offerings only
through clean packaging, benchmarks, trust, compatibility, and separately owned
hosted/enterprise products. Pricing, hosted document intelligence, enterprise
packaging, or roadmap changes require decision records backed by market and
customer analysis.

## Verification (narrow → wide)

```bash
bun install --frozen-lockfile
bun run check:github-actions
bun run check
bun run check:versions
cargo check -p anymd
bun run build && bun run test:cov
```
