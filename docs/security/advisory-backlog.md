# Advisory backlog and disposition

Originally recorded 2026-09-19; **updated 2026-09-20** after every open item was
resolved. Read with [maintainer-process.md](maintainer-process.md).

## Current state — no advisory in triage

| Advisory | State | Severity | Disposition |
| --- | --- | --- | --- |
| GHSA-f3xw-ff5r-rj7c | published 2026-07-17 | high 7.7 | IPv6 transition-address SSRF; fixed in 3.0.15 |
| GHSA-34gp-w56h-r2mv | published 2026-07-08 | high | url `file://` confinement bypass; fixed |
| GHSA-q344-5v34-gm84 | published 2026-06-25 | high | HTTP transport accepted unauthenticated requests; fixed in 3.0.1 |
| **GHSA-5r2f-7788-qp8v** | **published 2026-09-20** | medium | DNS-rebinding SSRF; fixed, regression-covered |
| **GHSA-rgg9-pwc3-jg39** | **published 2026-09-20** | medium | same defect, reported against the TS-default line |
| **GHSA-qr3f-g5wf-225w** | **published 2026-09-20** | medium | `--allow-dir` allowlist implemented and verified on the artifact |
| GHSA-886v-prww-cv4r | closed 2026-09-20 | high 7.5 | duplicate of GHSA-q344-5v34-gm84; closure recorded |
| GHSA-94pq-cpcq-m7j8 | closed 2026-09-20 | high 7.5 | duplicate; closure recorded |
| GHSA-392j-5r87-pwpp | closed 2026-09-20 | high 7.5 | duplicate; closure recorded |

Three advisories had been closed on 2026-07-08 with **no closure comment and no
published advisory**, so the public record showed nothing. They were re-opened to
triage on 2026-09-20, given the closure resolution they lacked, and closed again.

Reporter credit is recorded on every report. Credit transitions from `pending` to
`accepted` when the reporter accepts it in GitHub; that is the reporter's action,
not the maintainer's, and the API does not set it.

## What the 2026-09-19 record caught

An earlier version of this page recorded that the repository claimed "published
GHSAs are the security record" while four advisories sat in triage and two more
were closed without a record. That was accurate at the time and is kept in git
history. The rules it produced are now in
[maintainer-process.md](maintainer-process.md):

1. A closure comment is the record when the advisory window closes.
2. "Fixed" names an artifact, never a repository.
3. Triage is a state a product may not hide.

## Release-level findings (recorded, then resolved)

- `@sylphx/pdf-reader-mcp` **3.1.2** and **3.1.4** shipped the unpinned
  TypeScript loader as their entry point (`dist/index.js`). 3.1.0/3.1.1 shipped
  pinned Rust, 3.2.0 restored it. The version range on
  GHSA-5r2f-7788-qp8v (`>= 3.1.2, <= 3.1.4`) now names those releases.
- The `pdf-reader-*` crates on crates.io are yanked at their only version
  (3.1.1), so the advertised Rust install path resolves to nothing. The supported
  install path is the npm package; this repository should not advertise a crate
  that cannot be resolved until either is true.
