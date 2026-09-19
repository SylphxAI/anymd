# Advisory backlog (recorded 2026-09-19)

Read with [maintainer-process.md](maintainer-process.md). This page records what
was true on 2026-09-19 for `SylphxAI/pdf-reader-mcp` (Citra), because the state
was not visible anywhere else: the repository, the changelog, and the company
portfolio all implied the published advisories were the whole record.

## The nine advisories

| Advisory | State | Severity | Subject |
| --- | --- | --- | --- |
| GHSA-f3xw-ff5r-rj7c | published 2026-07-17 | high 7.7 | SSRF via IPv6 transition addresses (NAT64 / 6to4 / Teredo); fixed in 3.0.15 |
| GHSA-34gp-w56h-r2mv | published 2026-07-08 | high | `read_pdf` url source: SSRF and `file://` confinement bypass |
| GHSA-q344-5v34-gm84 | published 2026-06-25 | high | HTTP transport accepted unauthenticated requests despite `MCP_API_KEY` |
| GHSA-886v-prww-cv4r | **closed, never published** | high 7.5 | `MCP_API_KEY` documented and logged as enforced but never passed to the HTTP layer |
| GHSA-94pq-cpcq-m7j8 | **closed, never published** | high 7.5 | HTTP API key authentication bypass |
| GHSA-392j-5r87-pwpp | closed, never published | high 7.5 | `X-API-Key` silently unenforced → unauthenticated PDF disclosure |
| GHSA-5r2f-7788-qp8v | triage | medium | SSRF guard bypassable via DNS rebinding (TOCTOU) |
| GHSA-rgg9-pwc3-jg39 | triage | medium | DNS-rebinding SSRF in the default engine, reported against 5.0.0 |
| GHSA-qr3f-g5wf-225w | triage (two reports) | medium | `--allow-dir` filesystem allowlist not implemented in the pure-Rust server |

Three advisories were closed in a single hour on 2026-07-08 without a closure
comment or a published advisory. Two of those reporters' credits are still
`pending`.

## The DNS-rebinding report, read against the artifacts

GHSA-5r2f-7788-qp8v cites `src/pdf/loader.ts:104-129` and
`src/utils/config.ts:415-440`, and the unpinned `fetch` between them. Both files
are still on `main` and still contain exactly that defect. They are **residual
oracle code** — not shipped, not production authority, deleted when the oracle
migrates.

Checking the published artifacts instead:

| Artifact | URL loader | DNS pinned? |
| --- | --- | --- |
| `@sylphx/pdf-reader-mcp@3.1.0`, `@3.1.1` | Rust binary via `bin/pdf-reader-mcp` | yes |
| **`@sylphx/pdf-reader-mcp@3.1.2`, `@3.1.4`** | **`dist/index.js` (TypeScript)** | **no** |
| `@sylphx/pdf-reader-mcp@3.2.0`–`4.1.3` | Rust binary | yes |
| `@sylphx/citra@4.1.2`, `@5.0.0` | Rust binary | yes |
| `pdf-reader-core` crate | `PinnedResolver` in `crates/pdf-reader-core/src/url_fetch.rs` | yes |

So the reporter's file pointers were stale, the shipped engine was already
pinned, and the same defect was independently present in two published tarballs
during the 3.1.2/3.1.4 window — which no version range on the report expresses.

Every `pdf-reader-*` crate on crates.io is a single version, `3.1.1`, and is
**yanked**: the advertised Rust install path resolves to nothing.

## Resolution taken

`createPinnedAgent` in the residual loader now resolves each hop itself and pins
the approved answer into the connection (`{ all: true }` honoured, so the client
never re-resolves), and the module re-checks the answer against the same
non-public predicate. Regression coverage:
`test/pdf/rebind.test.ts` — proven to fail without the pin and pass with it.

## Open

- GHSA-5r2f-7788-qp8v, GHSA-rgg9-pwc3-jg39, GHSA-qr3f-g5wf-225w remain in
  **triage**.
- GHSA-886v-prww-cv4r, GHSA-94pq-cpcq-m7j8, GHSA-392j-5r87-pwpp remain closed
  without a published record; two credits `pending`.
- The three yanked crates and the 3.1.2/3.1.4 marking are release-surface
  decisions for the repository owner.
