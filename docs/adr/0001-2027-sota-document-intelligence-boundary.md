# ADR-0001: 2027 SOTA Document Intelligence Boundary

> Historical: decided for pdf-reader-mcp, now anymd, and kept as written. The package, crate, tool and file names it uses may no longer exist; anymd is one Rust binary in `crates/anymd`, and [AGENTS.md](../../AGENTS.md) has the current layout.

**Status:** Accepted
**Date:** 2026-06-16
**Deciders:** Kyle Tse, Codex
**Project:** pdf-reader-mcp

## Context

`@sylphx/pdf-reader-mcp` is a public MCP package that gives agents PDF reading
capability. It is not a Sylphx Platform BaaS service and should not become a
product-specific Spiron or Cubeage component. Its value comes from being a
reusable, benchmarked, provider-extensible, safe document-intelligence tool that
can be used by many agents and products.

The 2027 SOTA target is broader than text extraction. Agents increasingly need
layout-aware extraction, visual region analysis, page-level provenance,
streaming/batch behavior, robust error isolation, and public benchmark evidence.
Those capabilities must be added without turning the package into a hidden
hosted service or leaking customer documents into shared infrastructure.

## Decision

This repository owns the local/open-source document-intelligence package and
its public MCP contract.

It owns:

- MCP tools, schemas, package API, parser adapters, extraction options, page and
  region provenance, local file/URL handling, benchmarks, docs, and release
  evidence;
- provider-neutral document analysis interfaces for optional OCR, vision, or
  region-analysis providers;
- public compatibility with Claude Code, Claude Desktop, Cursor, VS Code,
  Windsurf, Cline, Warp, and other MCP clients.

It does not own:

- Sylphx Platform Durable Work, Auth, Billing, Storage, Gateway, hosted
  multi-tenant execution, customer accounts, or production data retention;
- Spiron/Cubeage product semantics;
- direct AI provider secrets or product-specific model routing.

If a hosted PDF/document-intelligence product is created, it must be a separate
service with its own ADR/spec and commercial controls. This package may become
the engine or SDK for that service, but it remains usable as a local package.

## Portfolio Integration Boundary

Sylphx Gateway, Spiron, Platform, or another product may register this package
as an external MCP tool source or consume its schemas through an adapter, but
that integration does not move the package boundary:

- `pdf-reader-mcp` remains the SSOT for local document-intelligence MCP tool
  schemas, provenance contracts, provider adapters, benchmarks, and release
  evidence.
- Gateway owns any model-facing manifest, native late-loading projection,
  tenant policy, hosted execution, cache diagnostics, and billing surface around
  registered tools.
- Product apps own product permissions, credential handles, audit, and durable
  state created after using the tool.
- Hosted multi-tenant document intelligence still requires a separate service
  ADR with auth, billing, storage, retention, quota, and privacy controls.

## SOTA Invariants

- The MCP schema is the public contract and must be versioned, documented, and
  regression-tested.
- Extraction output must carry page/region provenance so downstream agents can
  cite and verify source locations.
- Benchmarks and release gates must be reproducible and published with version
  context.
- Optional visual/OCR providers must be adapters behind a typed interface; no
  provider key or hosted credential belongs in the package.
- Local processing must not upload customer documents unless the caller
  explicitly selects a provider that requires remote processing.
- Errors are isolated per source/page/operation where possible; one bad page
  should not poison a whole batch unless the contract says so.
- Commercial value is built through trust: public docs, compatibility,
  benchmarks, security posture, release discipline, and optional hosted or
  enterprise offerings outside the local package boundary.

## Alternatives Considered

### Turn the package into a hosted BaaS service

Rejected. Hosted document intelligence may be valuable, but that requires
auth, billing, storage, retention, quotas, audit, and customer data controls
that do not belong inside this local MCP package.

### Keep the package as text-only extraction

Rejected. Text-only extraction is not 2027 SOTA for agent workflows. Layout,
images, regions, provenance, benchmarks, and provider adapters are required to
stay competitive.

### Add provider-specific code directly to tools

Rejected. Provider behavior must be isolated behind adapters so the public MCP
contract stays stable and users can choose local-only or remote-provider modes.

## Consequences

- Future implementation must prioritize schema stability, provenance, provider
  abstraction, benchmark evidence, and public docs.
- Hosted/commercial services must be separate products that reuse the package,
  not mutations of the local MCP runtime.
- Release readiness requires tests, typecheck, benchmark gates, docs, changelog,
  and package publish verification.
