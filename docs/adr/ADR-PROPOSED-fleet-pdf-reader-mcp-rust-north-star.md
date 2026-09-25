# ADR-PROPOSED — Fleet PDF Reader MCP Rust North Star architecture

> Historical: decided for pdf-reader-mcp, now anymd, and kept as written. The package, crate, tool and file names it uses may no longer exist; anymd is one Rust binary in `crates/anymd`, and [AGENTS.md](../../AGENTS.md) has the current layout.

> **Admission update (2026-07-22):** whole-product exact PDF.js/TS output parity is no longer the Rust release bar. See [ADR-0005](0005-capability-first-semantic-compatibility.md) for capability-first semantic compatibility. Frozen exact residual families remain regression assets.

- **Status:** Proposed
- **Date:** 2026-07-10
- **Relates to:** ADR-167 (SylphxAI/doctrine), ADR-0004 (reader portfolio architecture), SylphxAI/mcp-server-sdk
- **Change class:** `required-future` for PDF Reader MCP; `advisory` for fleet

## Context

PDF Reader MCP is a production local-first MCP package for PDF and document
intelligence: inspection, search, rendering, region crops, OCR routing,
extraction, document maps, trust signals, and provenance. Hosts connect via
**stdio** (default Rust `rmcp`) or **streamable HTTP** (`MCP_TRANSPORT=http`).
PDF Reader MCP owns the Reader portfolio architecture ADR (ADR-0004).

Rust crates (`anymd-core`, `anymd`)
implement parsers and tool engines behind `rmcp`. Transitional TS MCP adapter
and handlers remain until per-slice `ts_deleted`.

Doctrine [ADR-167](https://github.com/SylphxAI/doctrine/blob/main/docs/adr/ADR-167-boundary-contract-stack-and-platform-pillars.md)
requires Rust MCP server authority. North Star: native Rust `rmcp` binary with
benchmark-gated npm release — portfolio SSOT for reader boundaries.

## Decision

### 1. North Star production stack (PDF Reader MCP repo)

| Layer | North Star | Transitional (until sunset slice) |
| --- | --- | --- |
| Cross-boundary contract | Protobuf + Buf (`proto/pdf_reader/v1/`) for tool I/O | rmcp JSON Schema + golden fixtures |
| MCP transport (stdio) | Rust `anymd` via `rmcp::transport::stdio()` | TS adapter `src/index.ts` |
| MCP transport (HTTP) | Rust `http_transport.rs` (`rmcp::StreamableHttpService`) | TS Streamable HTTP opt-in |
| Tool handlers | Rust `anymd` → `anymd-core` | `src/handlers/*.ts` |
| PDF extraction / OCR routing | Rust `anymd-core` | TS engine during cutover |
| Reader portfolio contracts | `proto/` + ADR-0004 boundaries | Sibling repo delegation via public APIs |
| Distribution | Thin npm `bin/pdf-reader-mcp` → native binary | npm dist + transport opt-in |
| Deploy / packaging | CI-prebuilt native binary; benchmark-gated release | TS bundle in `dist/` |

### 2. Ownership matrix

| Concern | Owner | PDF Reader MCP may | PDF Reader MCP must not |
| --- | --- | --- | --- |
| PDF/document MCP tools, portfolio ADR | **SylphxAI/pdf-reader-mcp** | Own parser adapters, provenance, benchmarks | Become hosted Platform BaaS |
| Image reading | **SylphxAI/image-reader-mcp** | Delegate via portfolio contract | Own image OCR engine |
| Video reading | **SylphxAI/video-reader-mcp** | Delegate via portfolio contract | Own ffprobe/transcript pipeline |
| Unified read facade | **SylphxAI/smart-reader-mcp** | Expose tools to Smart Reader | Own format-sniff orchestration |
| MCP transport SDK | **SylphxAI/mcp-server-sdk** | Adopt `rmcp` + streamable HTTP | Own document parsing |

### 3. Strangler-fig cutover posture

- **S0:** Cargo workspace + rmcp stdio smoke; default npm bin launches Rust.
- **S1:** Core read tool golden parity (`read_pdf` and siblings) on `corpus/` fixtures.
- **S2:** Streamable HTTP transport parity; `check-no-ts-http-backend` gate.
- **S3:** Delete TS MCP adapter; Rust `rmcp` sole authority; benchmark release-gate
  transcript required.
- Each slice requires N4 + SHA-bound differential proof
  (`scripts/run-pdf-reader-differential.sh`) per rej-010.

### 4. Contract stack (ADR-167 alignment)

- **Protobuf + Buf** is SSOT for tool I/O and reader portfolio cross-boundary surfaces.
- **rmcp** + MCP JSON-RPC is the wire contract for stdio and streamable HTTP.
- Sibling reader repos consume portfolio proto — no hand-written parallel schemas.
- Local-first processing is the default; remote provider behavior is explicit and opt-in.

## Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| Permanent TypeScript MCP runtime | Contradicts ADR-167 for production reader authority |
| Monolithic reader repo | Rejected by ADR-0004 portfolio architecture |
| Hosted document SaaS | Violates PROJECT.md local-first boundary |

## Consequences

- New PDF logic defaults to `crates/anymd-core`.
- `proto/pdf_reader/v1/` is portfolio SSOT when cross-repo contracts land.
- TS `src/` adapter deleted per slice when parity + differential_green pass.
- Reader sibling repos align to portfolio proto contracts defined here.

## Validation

- `python3 $DOCTRINE/scripts/project-control-plane-audit.py --local . --fail-on-drift`
- Golden MCP tool parity on `corpus/` fixtures
- `scripts/run-pdf-reader-differential.sh` differential_green at bound SHAs
- `benchmark:release-gate` → stdio + HTTP transport authority probes
- `cargo test` + `cargo clippy -D warnings`
