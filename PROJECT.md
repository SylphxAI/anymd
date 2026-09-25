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
- Native optional packages: `@sylphx/anymd-<platformId>` (binary `anymd`)
- Compatibility aliases, published at the same version as `@sylphx/anymd`:
  `@sylphx/citra` (bin `citra`) and `@sylphx/pdf-reader-mcp` (bin `pdf-reader-mcp`)
  in `packages/alias-*`, kept in lockstep by `scripts/sync-alias-packages.ts`.
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
public MCP contract. Production backend authority is the pure-Rust crates and
native binary launched by the npm package entry. The TypeScript v3.0.14 engine
and its parity oracle are retired and deleted; TypeScript remains only as the
npm launcher and SDK wrapper.

It does not own hosted customer accounts, billing, storage, tenant policy,
Gateway routing, product audit, or durable state created after a tool is used.
Hosted document intelligence must be a separate service with its own spec
and commercial controls.

## Public Surfaces

- MCP package and CLI: `package.json` → `dist/runtime-entry.js` (native only)
- Rust core / server: `crates/pdf-reader-core`, `crates/pdf-reader-mcp-server`
- Public docs: `README.md`, `docs/`
- Tool/spec docs: `docs/specs/`
- CI: `.github/workflows/ci.yml`
- Release: Changesets + `.github/workflows/release.yml`

## Delivery

Terminal delivery is **npm package release** (main package + platform optional
native packages) with registry readback — not a hosted app deploy.

Pull requests use the legacy `Validate Code Quality` context on Sylphx
self-hosted runners. Darwin native package builds and registry proofs use
`[self-hosted, sylphx, macos, standard]` only — never GitHub-hosted `macos-*`.
Linux native ABI builds intentionally keep Ubuntu 22.04 images for GLIBC≤2.35;
Windows natives remain on `windows-latest` until a self-hosted Windows pool exists.
Package release is Changesets-driven through the repo release workflow, which
mints a GitHub App token before creating version PRs or publishing to npm.

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
bun run typecheck
bun run check
bun run check:ts-production-absence
bun run package:smoke
```
