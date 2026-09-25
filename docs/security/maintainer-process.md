# Security advisory maintainer process

Audience: whoever holds this repository's security advisories. It exists because
the whole record — portfolio, changelog, and this doc — was wrong once: see
[the advisory backlog audit](advisory-backlog.md).

## Rules

### 1. A closure comment is the record

GitHub allows a private advisory to be **published** only inside its window
(24 hours by default; a **triage** report has a longer one). Past the window,
that advisory can still be **closed** — and a closed advisory with no published
advisory is an unresolved disclosure: the reporter never receives credit, the
public never sees a record, and the defect looks fixed because the row says
`closed`.

So:

- **Closing without publishing requires a closure comment** that names (a) the
  advisory that supersedes it, or (b) an issue, commit, or release that resolves
  it. "Duplicate" without a link is not a closure comment.
- **A closed advisory whose reporter credit is still `pending` is unresolved.**
  Accepted credit is the proof the reporter was acknowledged.
- Prefer publishing while the window is open. If the window has passed, file the
  record on `main` and reply to the private report with the resolution.

### 2. "Fixed" names an artifact

`fixed` and `unfixed` are claims about a **specific artifact**, never about the
repository as a whole. State which one:

| Artifact | Example |
| --- | --- |
| Shipped npm tarball | `@sylphx/anymd@5.0.1`, entry `dist/runtime-entry.js` |
| Native optional package | `@sylphx/anymd-linux-x64-gnu@5.0.1`, binary it launches |
| Source tree | `crates/pdf-reader-core/src/url_fetch.rs` at `v4.1.3` |
| Residual / oracle code | not shipped; not production authority |

A defect can be fixed in the shipped binary and still present in the source tree,
or fixed in source and still present in an older published tarball. Both are
real, and each needs its own sentence.

### 3. Triage is a state a product may not hide

An advisory in `triage` is **unreviewed**, not absent. A product's security
record includes every state: `triage`, `draft`, `published`, and `closed`. When a
release closes, list the open advisories — including ones you decided not to fix
— so the next reader is not told the record is clean.

## Verifying a report's file pointers

A report written against a source checkout may cite a path no published artifact
uses. Before acting, confirm which artifact serves the code:

1. `npm view <pkg> dist.tarball` for the published tarball; unpack it and read
   `package.json` `bin` / `exports` to find the entry point.
2. Check whether the named code is in the shipped entry point, in residual
   source, or in neither.
3. Only then decide: fix, delete, or record as superseded.

## Known vulnerable releases

Kept here because an advisory's single version range cannot express a defect
that returned for two releases mid-migration. See
[advisory-backlog.md](advisory-backlog.md) for the evidence.

| Release | Why | Fixed in |
| --- | --- | --- |
| `@sylphx/pdf-reader-mcp@3.1.2` | entry point `dist/index.js`; TypeScript URL loader, no DNS pinning | `@sylphx/pdf-reader-mcp@3.2.0` |
| `@sylphx/pdf-reader-mcp@3.1.4` | same entry point and defect | `@sylphx/pdf-reader-mcp@3.2.0` |
