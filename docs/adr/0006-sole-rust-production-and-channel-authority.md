# ADR-0006 — Sole-Rust production authority and distribution channels

> Historical: decided for pdf-reader-mcp, now anymd, and kept as written. The package, crate, tool and file names it uses may no longer exist; anymd is one Rust binary in `crates/anymd`, and [AGENTS.md](../../AGENTS.md) has the current layout.

- **Status:** Accepted
- **Date:** 2026-07-24
- **Relates to:** ADR-0005 capability-first admission, npm package contract, MCP Registry, crates.io
- **Change class:** `required-now` for the showhand sole-Rust release (SemVer major; target `4.0.0`)

## Context

`@sylphx/pdf-reader-mcp@3.2.2` is a transitional **Rust-default** release:

- default entry launches the platform native binary and fails closed if missing;
- TypeScript remains an explicit production path (`./typescript`, force flags);
- the published tarball still ships `dist/index.js`, `legacy-engine-runtime.js`, and PDF.js workers.

That is **not** Sole Rust. Marketing or matrix language that equates “default points at Rust” with “TypeScript production retired” is false.

crates.io previously published experimental pure-Rust crates (later yanked/withdrawn). Keeping crates as an implied product channel without version-aligned admission creates contradictory public truth.

## Decision

### 1. Sole Rust means Rust owns all production PDF intelligence

For the next stable product line (major `4.0.0`):

- Rust owns stdio, HTTP, MCP tools, PDF processing, Document Twin, evidence, OCR/provider routing, trust/accessibility, and failure behavior.
- The npm umbrella may ship **only** a thin JavaScript launcher that selects and executes the exact platform-native binary (plus optional thin spawn helpers that do not process PDFs).
- No TypeScript PDF processing runtime may be built into, shipped in, imported by, or invoked from the production package.
- Public `./typescript` export, force-TS flags, `dist/index.js`, `legacy-engine-runtime.js`, and PDF.js worker payloads are **removed** from the production artifact.
- TypeScript `3.0.14` remains an **immutable external LKG / oracle** version and test-only material in the repository. It is not bundled into the sole-Rust production package.

### 2. Removing `./typescript` is a breaking public-contract change

Default SemVer outcome is **major** (`4.0.0`). Do not publish another stable minor/patch that claims Sole Rust while retaining TS production payloads.

### 3. crates.io is not an admitted product distribution channel

Unless a future ADR re-admits crates with versions aligned to the npm product line and full admission gates:

- crates.io is **not** a supported install path for end users;
- `publish-crates.yml` must refuse product publishes;
- public docs must not promise crates as a drop-in product channel.

Primary admitted product channels for the showhand release:

1. npm (`@sylphx/pdf-reader-mcp` + platform optional natives)
2. GitHub Release/tag identity
3. MCP Registry metadata pointing at the npm package

### 4. 3.2.2 handling

- Do **not** unpublish 3.2.2 merely for narrative cleanliness.
- Document it honestly as transitional Rust-default with bundled TS rollback.
- Keep `3.0.14` as external rollback until sole-Rust `4.0.0` is proven and published.
- Do not move `latest` to sole-Rust until exact-candidate gates + independent review pass.

### 5. Performance claims require same-host controlled A/B

Historical cross-run Rust vs TS numbers are not marketing claims. Showhand performance claims require a controlled same-host A/B harness against immutable TS `3.0.14` with bound SHA, binaries, fixtures, and raw samples.

## Consequences

- Package manifests, build, tarball smoke, runtime probes, and CI must mechanically prove TS production absence.
- Capability-first admission (ADR-0005) remains the quality bar; Sole Rust does not weaken capability requirements.
- Independent whole-product review must authorize sole-Rust production authority, TS deletion, performance claims, and goal-complete eligibility before stable publish.

## Outcome (2026-09-20)

Section 3 is executed, not merely decided:

- **crates.io carries nothing resolvable.** `pdf-reader-core`,
  `pdf-reader-mcp-server`, and `pdf-reader-cli` each have exactly one published
  version (`3.1.1`) and it is **yanked**. The advertised `cargo install` path
  resolves to nothing, so it is not a channel anyone can actually use.
- **`docs/performance/why-rust.md` no longer lists crates.io** as a product
  channel; it names the npm package.
- **The admitted channels are the three in §3**: npm `@sylphx/citra` plus
  platform optional natives, GitHub Release/tag identity, and the MCP Registry
  entry `io.github.SylphxAI/citra` (published, `active`, pointing at the npm
  package at the same version).
- `publish-crates.yml` stays behind its explicit `PUBLISH FREEZE` confirmation
  and publishes nothing on an ordinary run, which is the refusal §3 requires.

Re-admitting crates requires a future ADR with versions aligned to the npm line
and the same admission gates; until then the yanked versions are the correct
state, not an oversight to fix.
