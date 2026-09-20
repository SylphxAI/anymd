# Advisory resolution against the published 5.0.2 artifacts

Recorded 2026-09-20 against the **published** artifacts, not source. Read with
[advisory-backlog.md](advisory-backlog.md) and
[maintainer-process.md](maintainer-process.md).

Every claim below was executed against the registry artifact. Where a report
cited a source path, the test says which artifact actually serves that code.

## GHSA-qr3f-g5wf-225w — `--allow-dir` allowlist inert in the pure-Rust server

**Status: fixed in the published artifact. Verified by execution.**

```
$ citra-mcp-server --allow-dir=/tmp/allowtest/allowed < read-outside.json
{"jsonrpc":"2.0","id":2,"error":{"code":-32602,
 "message":"Access denied: path '/tmp/allowtest/outside/secret.pdf'
            is outside the configured allowed directories."}}
```

Without the flag the same call proceeds to parse the file (the run returns a PDF
parse error, not an access error), so the flag is the thing making the
difference. The published binary announces it in `--help`:

```
--allow-dir=<path>       Restrict local PDFs to this directory (repeatable)
MCP_PDF_ALLOWED_DIRS     Platform path-list of allowed directories
```

Run against `@sylphx/citra-linux-x64-gnu@5.0.2`. The report tested a locally
built HEAD binary and said so; HEAD has since implemented the allowlist.

## GHSA-rgg9-pwc3-jg39 and GHSA-5r2f-7788-qp8v — DNS-rebinding SSRF

**Status: fixed in every artifact that can serve the vulnerable code path.**

Both reports describe the same defect: the guard resolves the hostname, then the
fetch resolves it again, so a hostile zone can answer the check publicly and the
connect privately.

| Artifact | URL path | Verdict |
| --- | --- | --- |
| `@sylphx/citra@5.0.2` | Rust binary (`PinnedResolver`) | fixed |
| `@sylphx/pdf-reader-mcp@4.1.3` (its `latest`) | Rust launcher → native binary | fixed — no TS loader shipped |
| `@sylphx/pdf-reader-mcp@3.1.2`, `@3.1.4` | `dist/index.js` (TypeScript) | **vulnerable, and no longer resolvable as `latest`** |
| residual `src/pdf/loader.ts` | not shipped | fixed in `#689` for completeness |

`@sylphx/pdf-reader-mcp@4.1.3` is a Rust launcher with zero occurrences of the
TypeScript loader (`grep -c validateUrlHop dist/runtime-entry.js` → `0`). The
report's premise — "the default install runs the vulnerable TS engine" — was
true for the window it tested and is not true of what the registry serves now.

## What is still open

These are the maintainer decisions this record cannot make for the reporter:

1. The three advisories above remain in **triage**. Publishing them, and moving
   the reporter's credit from `pending` to accepted, are actions on someone
   else's report.
2. `GHSA-886v-prww-cv4r`, `GHSA-94pq-cpcq-m7j8`, `GHSA-392j-5r87-pwpp` were
   closed with no published advisory and no fix record; two credits are still
   `pending`. A closure comment naming the resolution is still owed.
3. `@sylphx/pdf-reader-mcp@3.1.2` and `@3.1.4` should be marked vulnerable, or
   the whole transitional line deprecated, so no one installs the TS engine
   believing it is the product.
4. The `pdf-reader-*` crates on crates.io are yanked (3.1.1, their only
   version). Either un-yank, or state on the repository that the Rust install
   path is npm-only.
