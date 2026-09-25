# RETIRED family control-plane note

> Historical: decided for pdf-reader-mcp, now anymd, and kept as written. The package, crate, tool and file names it uses may no longer exist; anymd is one Rust binary in `crates/anymd`, and [AGENTS.md](../../AGENTS.md) has the current layout.

Superseded by product-local POSITIONING/IPPB. Historical only.

# ADR-389: Adopt PDF Reader MCP Family SOTA Roadmap

Date: 2026-07-09
Status: Accepted
Slug: mcp-family-sota-roadmap

## Context

PDF Reader MCP is the mature document evidence engine in the SylphxAI Reader
family. It needs a repo-local roadmap that preserves its local-first document
intelligence boundary while showing how PDF evidence feeds Smart Reader,
Architecture Reader, Consultant MCP, and other family workflows.

## Decision

Adopt `docs/roadmap/sota-family-roadmap.md` as the local roadmap for PDF Reader
MCP's family role.

PDF Reader MCP remains the owner of PDF document evidence, Agent Document Twin
semantics, provenance, trust reports, search, rendering, crops, OCR routing,
and benchmark-gated release quality.

## Consequences

- Smart Reader routes PDFs but does not replace PDF Reader internals.
- Architecture Reader and Consultant MCP consume PDF evidence rather than
  duplicating PDF extraction.
- Rust native acceleration and Rust MCP serving may be added only where
  benchmarks and fixtures preserve existing tool semantics and improve measured
  hot paths.
- Provider routes, privacy behavior, confidence, and degraded extraction must
  stay explicit.

## Amendment: Rust-Native MCP Runtime

The family runtime direction now targets Rust MCP servers using
`modelcontextprotocol/rust-sdk` / `rmcp`. PDF Reader MCP may keep TypeScript
compatibility wrappers during migration, but the target MCP server runtime is
Rust with the existing `read_pdf`, `search_pdf`, and `pdf_evidence` contracts.

## Verification

- Roadmap added at `docs/roadmap/sota-family-roadmap.md`.
- README and PROJECT link to the roadmap.
- Docs-only validation: `git diff --check`.
