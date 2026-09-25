# 2027 SOTA Document Intelligence Operating Model

> Historical: written for pdf-reader-mcp, now anymd, and kept as written. Its tools `read_pdf`, `search_pdf` and `pdf_evidence` became `read`, `search` and `inspect` (see [the tools guide](../guide/tools.md)); files and commands named here may no longer exist.

Date: 2026-06-16
Status: Implementation target
Owner: pdf-reader-mcp
Related: `docs/adr/0001-2027-sota-document-intelligence-boundary.md`

## Goal

Make `@sylphx/pdf-reader-mcp` a public, commercial-grade quality,
state-of-the-art document-intelligence MCP package for agents, while preserving
local-first privacy and a clean boundary from hosted Platform services.

## Non-Goals

- Do not add hosted auth, billing, storage, tenancy, or data retention inside
  the package.
- Do not add direct provider secrets or Sylphx AI Gateway credentials to the
  package.
- Do not optimize for one agent client at the expense of MCP contract
  compatibility.
- Do not claim release readiness without docs, tests, benchmarks, and changelog
  evidence.

## Capability Map

| Area | SOTA requirement |
|------|------------------|
| MCP contract | Stable tool schemas, versioned options, compatibility fixtures, generated/API docs. |
| Text extraction | Page-aware and full-document extraction with deterministic ordering and clear whitespace policy. |
| Layout/provenance | Page, bounding region, image, table, and source metadata sufficient for citations and verification. |
| Visual/OCR adapters | Optional provider interface for OCR and region analysis with local-only default behavior. |
| Batch processing | Multiple sources, partial failure isolation, concurrency limits, cancellation-safe behavior. |
| Performance | Reproducible benchmarks by document class, page count, image density, and operation type. |
| Security/privacy | Path handling, URL handling, file size limits, remote-processing opt-in, no secret logging. |
| Release quality | Typecheck, tests, benchmark gate, docs build, changelog, npm package smoke, compatibility smoke. |

## Portfolio Integration Rules

- Gateway may register this package as an external MCP tool source, but Gateway
  owns any model-facing manifest, native late-loading projection, tenant policy,
  hosted execution, cache diagnostics, billing, and usage surface around that
  registration.
- Product apps own product permissions, credential handles, audit, and durable
  outcomes created after using these tools.
- A hosted multi-tenant document-intelligence product is a separate service,
  not a mutation of this local MCP package.

## Provider Boundary

Provider adapters may exist for OCR, vision, or layout analysis, but they must:

- be optional dependencies or optional runtime adapters;
- accept credential handles from the caller environment, never hardcoded keys;
- declare whether documents leave the local machine;
- return typed, provenance-preserving results;
- expose cost/latency/error metadata where available;
- fail per page/source when possible.

## Commercial Paths

The local package remains free/open-source quality software. Commercial value
can be created through:

- a hosted document-intelligence service in a separate repo/service with
  Platform Auth/Billing/Storage/Durable Work controls;
- enterprise support and compatibility certification;
- benchmark-backed performance claims;
- private provider adapters or compliance packs;
- integration bundles for agent platforms and CI/document workflows.

## Acceptance Criteria

- Every public tool option is covered by schema tests and docs.
- Extraction results include source and page provenance.
- Visual/OCR provider support is adapter-based and local-only remains the
  default.
- Benchmarks are reproducible and versioned with hardware/runtime context.
- Release gate runs typecheck, tests, package build, benchmark release gate,
  docs build, and npm smoke where credentials permit.
- A failed page/source does not hide successful extraction from other pages or
  sources unless the caller requested fail-fast behavior.
- The README, docs site, changelog, and `progress.md` agree before release.

## Implementation Slices

1. Add schema-level contract fixtures for existing tools.
2. Add provenance normalization for pages, images, and future regions.
3. Add provider adapter interface for visual/OCR analysis without bundled
   provider secrets.
4. Add benchmark release gate that separates small, scanned, image-heavy,
   table-heavy, and large-document classes.
5. Add docs explaining privacy modes, local-only behavior, remote-provider
   opt-in, and commercial hosted boundary.
6. Add release checklist that blocks publish when docs, benchmarks, changelog,
   and package smoke are out of sync.
