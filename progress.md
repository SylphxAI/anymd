# Progress

## Status: Production package (sole-Rust)

Current published product line: `@sylphx/pdf-reader-mcp@4.x` with platform
optional native binaries. Production authority is the Rust MCP server; npm entry
is a fail-closed launcher only.

Adoption is **migrating** (see `project.manifest.json` gaps): residual TypeScript
oracle surface, npm vs Cargo version skew documentation, and trunk CI health.

## Completed

- [x] Core MCP tools: `read_pdf`, `search_pdf`, `pdf_evidence`
- [x] Local path and URL sources with access restrictions
- [x] Agent Document Twin outputs (structure, provenance, trust/accessibility)
- [x] Sole-Rust production path with fail-closed missing native binary
- [x] npm package + optional native platform packages
- [x] Release through the shared mcp-kit workflow (8.0.0)
- [x] Capability parity, agent-task evals, host/registry proof artifacts
- [x] Project control: Skills SSOT + project-manifest v2 (Doctrine adapter removed)

## In Progress

- [ ] Keep default-branch CI green (`bun install --frozen-lockfile` + Validate Code Quality)
- [ ] Isolate or delete residual TypeScript PDF oracle surface
- [ ] Machine-readable npm ↔ Cargo version identity mapping

## Planned

- [ ] Optional advanced parser engine adapters (behind explicit contracts)
- [ ] Large-file streaming beyond the current size cap
- [ ] Broader public scanned-PDF / visual-region accuracy corpus (licensed)
