# Release candidacy — 4.1.0 product performance

> Historical: a pdf-reader-mcp release record, kept as written. The `verification/` files it names are no longer in this repository; anymd releases are described in [PUBLISH.md](../PUBLISH.md).

Status: **candidate preparation** (not published)  
Base: sole-Rust live `@sylphx/pdf-reader-mcp@4.0.2` remains production until 4.1.0 is admitted.

## Why 4.1.0 (minor)

User-visible product improvements beyond packaging:

1. Process-local warm cache for identical local `read_pdf` requests (agent multi-call latency)
2. Release binary profile (`strip` + thin LTO) for smaller natives on next publish
3. Public product proof page + before/after acquisition assets
4. Honest dual-mode performance surfaces documented for marketing authorization

## Required admission before npm latest moves

1. Exact-head independent review of the versioned release SHA
2. Dual-mode suite on **registry-installed** exact 4.1.0 natives
3. `performanceClaimsAuthorized` only with method bounds:
   - startup_inclusive labeled spawn+init+task
   - persistent_warm labeled long-lived + may include process-local identical-request cache
   - first request still pays full parse cost
4. Five-host registry install/runtime proof
5. README/npm/site truth aligned

## Must not claim until authorized

- Universal multi-host Nx faster
- OCR/provider external I/O speed
- That 4.0.2 already includes warm cache (it does not)

## Keep live until cutover

Prefer `@sylphx/pdf-reader-mcp@4.0.2` until 4.1.0 registry proofs complete.

## Baseline evidence (published 4.0.2 registry)

`verification/pdf-reader-same-host-ab-suite-registry-4.0.2-baseline.json`:

- `rustFromRegistry: true`
- status: `measured_draft_not_admissible`
- persistent_warm fails 1.5× gate on several classes (no warm-cache in 4.0.2 binary)
- native binary ~25.5 MB (pre-strip profile)

Local main with warm-cache: `verification/pdf-reader-same-host-ab-suite-warm-cache-draft.json` status `admissible_pass`.
