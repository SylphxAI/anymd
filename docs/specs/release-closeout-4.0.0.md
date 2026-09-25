# Release closeout — @sylphx/pdf-reader-mcp@4.0.0

> Historical: a pdf-reader-mcp release record, kept as written. The `verification/` files it names are no longer in this repository; anymd releases are described in [PUBLISH.md](../PUBLISH.md).

Date: 2026-07-24  
Status: **channels live; performance claims withheld**

## Exact identities

| Surface | Value |
| --- | --- |
| npm latest | `4.0.0` |
| npm gitHead | `8063e4b78c3d77116fa6430f1804f0967984daf8` |
| implementation candidate | `f9a31541c7083eda6efe3fc828a6223a8c476342` |
| independent review | `verification/pdf-reader-whole-product-independent-review-4.0.0-f9a3154.json` |
| main land (squash) | `096b0259e599b0a16e3c7ba2a3403cee974a4617` |
| GitHub Release | [v4.0.0](https://github.com/SylphxAI/pdf-reader-mcp/releases/tag/v4.0.0) |
| MCP Registry | `io.github.SylphxAI/pdf-reader-mcp@4.0.0` active + `isLatest=true` |
| five-host registry proof | [run 30070323043](https://github.com/SylphxAI/pdf-reader-mcp/actions/runs/30070323043) |

## Sole Rust truth

- Production package ships thin launcher only (`dist/runtime-entry.js`, `dist/pure-rust.js`)
- No `./typescript` export; force-TS fails closed
- No TS/PDF.js production payload in npm tarball (14 files)
- Missing native package fails closed with external LKG guidance for `@3.0.14`

## Performance

Marketing speedups remain **withheld**. See
`docs/specs/performance/4.0.0-performance-claims-policy.md`.

## Rollback

- Historical TS production: `@sylphx/pdf-reader-mcp@3.0.14`
- Transitional Rust-default+bundled-TS history: `3.2.2` (not Sole Rust)
