# Release closeout — @sylphx/pdf-reader-mcp@4.0.2

> Historical: a pdf-reader-mcp release record, kept as written. The `verification/` files it names are no longer in this repository; anymd releases are described in [PUBLISH.md](../PUBLISH.md).

Date: 2026-07-24  
Status: **channels live; Sole-Rust packaging proven; formal performance/goal complete reopened after post-publish audit**

## Live identities

| Surface | Value |
| --- | --- |
| npm latest | `4.0.2` |
| npm gitHead | `6fd429558955d31b1310932640b2f316bf0dc299` |
| implementation candidate | `afb1aa3387dc3b4e10b9415d31dfea8844b3a89a` |
| independent review (packaging) | `verification/pdf-reader-whole-product-independent-review-4.0.2-afb1aa3.json` |
| historical perf/goal review (superseded by audit) | `verification/pdf-reader-whole-product-independent-review-4.0.2-perf-16fc2eb.json` |
| GitHub Release | [v4.0.2](https://github.com/SylphxAI/pdf-reader-mcp/releases/tag/v4.0.2) |
| MCP Registry | `4.0.2` active + isLatest |
| five-host registry proof | [run 30092324925](https://github.com/SylphxAI/pdf-reader-mcp/actions/runs/30092324925) |

## Proven

- Empty production `dependencies`
- Clean install has **no** `pdfjs-dist` / MCP TS SDK
- All five native tarballs HTTP 200 and registry install+initialize green
- Force-TS fail-closed; missing native fail-closed
- Prefer **4.0.2** over 4.0.1 (`linux-x64-gnu@4.0.1` tombstoned)

## Not complete / not authorized

- Formal product-wide performance claims (`performanceClaimsAuthorized=false`)
- Goal complete (`goalCompleteAuthorized=false`)
- Historical suite measured startup-inclusive end-to-end samples with a weak semantic gate
- Customer README on npm tarball still older than post-audit GitHub README until a future admitted patch publishes docs

## Next closeout actions

1. Land customer-first README + matrix honesty + dual-mode perf harness
2. Re-admit performance only under rebuilt `persistent_warm` contract (or clearly labeled startup claims)
3. Successor independent review after evidence
4. If npm README must match, publish next admitted patch version
