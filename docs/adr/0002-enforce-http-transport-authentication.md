# ADR-0002: Enforce HTTP Transport Authentication (X-API-Key)

**Status:** Accepted
**Date:** 2026-06-25
**Deciders:** Kyle Tse, Claude
**Project:** pdf-reader-mcp, now anymd

> The TypeScript files named below (`src/index.ts`, `src/mcp.ts`) no longer exist.
> The decision still holds: `crates/anymd/src/http_transport.rs` enforces it in Rust.

## Context

A security researcher (handle "novice-22") privately reported that the HTTP
transport (`MCP_TRANSPORT=http`) accepted unauthenticated requests even when the
operator configured an API key.

The defect was confirmed in code, including the current `v3.0.0`:

- `src/index.ts` read `MCP_API_KEY` into a variable but never passed it to the
  transport, and logged `API key authentication enabled (X-API-Key header)` at
  startup regardless.
- `src/mcp.ts` (the local HTTP layer built on `node:http` +
  `@modelcontextprotocol/sdk`'s `StreamableHTTPServerTransport`) had no
  authentication check on `/mcp`.
- `MCP_HTTP_HOST` defaulted to `0.0.0.0`, binding all interfaces.

Result: any client that could reach the port could call every PDF tool and read
any PDF the process could open — while the startup log falsely asserted the
endpoint was authenticated. The silent, misleading affirmation is the
aggravating factor: an operator who set `MCP_API_KEY` reasonably believed the
server was protected. (CWE-306 Missing Authentication for a Critical Function →
CWE-200 Information Disclosure.)

### Ownership / boundary

The vulnerability is owned by **this package**, not by an SDK. The bug is a
*false promise*: pdf-reader-mcp advertised and logged an authentication control
it never enforced. As of `v3.0.0` the package no longer depends on
`@sylphx/mcp-server-sdk`; it owns its HTTP request handling directly in
`src/mcp.ts`, so the fix is fully in-repo (see [ADR-0001](0001-2027-sota-document-intelligence-boundary.md)
for why this package stays standard-SDK-based and dependency-light).

A separate, lesser observation — that `@sylphx/mcp-server-sdk`'s `http()`
transport exposes no auth option — is a capability gap in that SDK, not this
CVE, and is out of scope here because v3 does not use that SDK. If that SDK is
still consumed elsewhere, closing that gap belongs in the SDK's own repo.

## Decision

1. **Enforce `X-API-Key`.** When `MCP_API_KEY` is set, every data-bearing
   `/mcp` request must present a matching `X-API-Key` header; missing or
   mismatched keys are rejected with `401`. The comparison is constant-time
   (SHA-256 + `timingSafeEqual`) so it leaks neither the key nor its length via
   timing. `/mcp/health` and CORS preflight (`OPTIONS`) stay open — the former
   carries no PDF data, the latter no credentials.
2. **Default to loopback.** `MCP_HTTP_HOST` defaults to `127.0.0.1`. Exposing
   other interfaces is an explicit opt-in.
3. **Tell the truth at startup.** The "authentication enabled" line prints only
   because the key is now actually enforced. When the server binds a
   non-loopback host with no key, it logs a prominent warning.
4. **Encode the guarantee as a CI gate.** Integration tests assert `401` without
   a key, `401` with a wrong key, `200` with the correct key, that `tools/list`
   is refused unauthenticated, and that `/mcp/health` stays open.
5. **Disclose.** Publish a GitHub Security Advisory (GHSA, CVE where it
   qualifies) crediting "novice-22", and add a `SECURITY.md` that routes future
   reports through GitHub private vulnerability reporting (the prior
   `hi@sylphx.com` channel was bouncing under the domain's MTA-STS policy).

Filesystem confinement stays opt-in (`--allow-dir` / `MCP_PDF_ALLOWED_DIRS`) for
now; defaulting to a restrictive allowlist is a separate, more disruptive change
tracked outside this ADR.

## Affected and fixed versions

- **Affected:** `>=2.2.0` (the release that introduced the HTTP transport)
  through `3.0.0`.
- **Fixed:** the next patch release (`3.0.1`). Fix-forward only; no backport to
  the `2.x` line.

## Alternatives considered

### Only fix the misleading log

Rejected. Removing the false claim without enforcing the key still leaves the
endpoint open; operators want the control to work, not just to stop lying.

### Add an auth option to `@sylphx/mcp-server-sdk`

Moot for this CVE. `v3.0.0` does not depend on that SDK, and coupling a public,
portable package back to an in-house SDK would contradict ADR-0001.

### Default to a deny-by-default filesystem allowlist

Deferred. Worth doing, but it breaks existing single-tenant users who rely on
unrestricted local reads and is orthogonal to the authentication defect.

## Consequences

- **Breaking for some deployments.** Servers that relied on the `0.0.0.0`
  default must now set `MCP_HTTP_HOST` explicitly. Clients that talk to a
  key-protected server must send the `X-API-Key` header — which is the point.
- Operators who already set `MCP_API_KEY` are protected for the first time.
- The HTTP transport now has authentication regression coverage; a future change
  that drops enforcement fails CI.
- Future hosted/commercial offerings still live outside this package boundary
  (ADR-0001); this change does not turn the package into a hosted service.
