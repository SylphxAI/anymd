# Changelog

## 5.0.2

### Patch Changes

- [#689](https://github.com/SylphxAI/pdf-reader-mcp/pull/689) [`7e0eaea`](https://github.com/SylphxAI/pdf-reader-mcp/commit/7e0eaeab5bf377c5aafb26ce40888d86a4adfa8b) Thanks [@shtse8](https://github.com/shtse8)! - Pin the URL loader's connection to the DNS answer the SSRF guard validated, so a
  hostile zone can no longer answer the check with a public address and the connect
  with `169.254.169.254` (DNS-rebinding / TOCTOU bypass, GHSA-5r2f-7788-qp8v). Adds
  `test/pdf/rebind.test.ts`, which fails without the pin.

- [#676](https://github.com/SylphxAI/pdf-reader-mcp/pull/676) [`d41b574`](https://github.com/SylphxAI/pdf-reader-mcp/commit/d41b574c6d69172c3bd7c57db66f888a5052d982) Thanks [@shtse8](https://github.com/shtse8)! - Contain malformed inline-image (`BI`/`ID`/`EI`) content-stream failures in PDF text extraction so a damaged or adversarial page returns a structured `invalid content stream (page N)` extraction error instead of aborting the native server.

## 5.0.1

### Patch Changes

- [#644](https://github.com/SylphxAI/pdf-reader-mcp/pull/644) [`3324cf8`](https://github.com/SylphxAI/pdf-reader-mcp/commit/3324cf8a31c57754b1ae9b743f6067a1e3e77742) Thanks [@shtse8](https://github.com/shtse8)! - Bind the production launcher to the matching native package version and prove
  the installed launcher, upgrade, and uninstall customer paths before release
  closeout.

- [#665](https://github.com/SylphxAI/pdf-reader-mcp/pull/665) [`9409b5b`](https://github.com/SylphxAI/pdf-reader-mcp/commit/9409b5b190d506abb9a68ed26df7d046fc0de39e) Thanks [@shtse8](https://github.com/shtse8)! - Contain malformed CFF Custom-encoding failures in PDF text extraction so a
  malicious or damaged Type1C font returns a structured extraction error instead
  of aborting the native server.

- [#666](https://github.com/SylphxAI/pdf-reader-mcp/pull/666) [`abe0702`](https://github.com/SylphxAI/pdf-reader-mcp/commit/abe070293e501370255005462f3e0a293a98cea0) Thanks [@shtse8](https://github.com/shtse8)! - Upgrade rmcp to 3.1.2 and add MCP 2026-07-28 result envelope support (cacheScope, ttlMs, serverInfo \_meta) for tools/list and tools/call.

- [#643](https://github.com/SylphxAI/pdf-reader-mcp/pull/643) [`984d2f9`](https://github.com/SylphxAI/pdf-reader-mcp/commit/984d2f95965300493b893d20feafb9bb1c014541) Thanks [@shtse8](https://github.com/shtse8)! - Enforce the documented native filesystem allowlist for every PDF tool, update
  the PDF parser dependency chain past RUSTSEC-2026-0187, and retire the unbound
  Docker publication path.

## 5.0.0

### Breaking

- Brand-sole package identity: `@sylphx/citra` is canonical (bin `citra`, MCP `io.github.SylphxAI/citra`).
- Transitional `@sylphx/pdf-reader-mcp` is no longer the install CTA.
- Family Evidence Envelope v1 adopted as wire law (skills schema).
- Prism composition product retired; host routes media.

## 4.1.3

### Patch Changes

- [#609](https://github.com/SylphxAI/pdf-reader-mcp/pull/609) [`0096f4c`](https://github.com/SylphxAI/pdf-reader-mcp/commit/0096f4c4d2e77413c34b4ab957127c2ccdd329f3) Thanks [@shtse8](https://github.com/shtse8)! - Fix a whole-process crash when reading PDFs whose ToUnicode CMaps contain malformed destinations (pdfTeX 1-byte `beginbfrange` like `<C5> <D6> <C5>`). Vendor a patched `adobe-cmap-parser` that skips destinations that are not valid UTF-16BE instead of panicking, keep multi-code (6/8-byte) ligature destinations working, and build with panic-unwind so no malformed document can abort the MCP server from a worker-thread panic (fixes [#608](https://github.com/SylphxAI/pdf-reader-mcp/issues/608)).

## 4.1.2

### Patch Changes

- [#599](https://github.com/SylphxAI/pdf-reader-mcp/pull/599) [`4bc0801`](https://github.com/SylphxAI/pdf-reader-mcp/commit/4bc0801471f1eb3e89896c2858031363a930621d) Thanks [@shtse8](https://github.com/shtse8)! - Fix Gemini Antigravity / dual-era MCP clients that send `server/discover` before `initialize`: answer SEP-2575 discovery without closing stdio, then complete the legacy handshake (fixes [#598](https://github.com/SylphxAI/pdf-reader-mcp/issues/598)).

## 4.1.1

### Patch Changes

- Sync npm README with authorized 4.1 dual-mode performance claims and product proof wording.

## 4.1.0

### Minor Changes

- [#591](https://github.com/SylphxAI/pdf-reader-mcp/pull/591) [`66e73cb`](https://github.com/SylphxAI/pdf-reader-mcp/commit/66e73cb33a8c79d8f268b5d358205b932ae13e00) Thanks [@shtse8](https://github.com/shtse8)! - Product performance release: process-local warm `read_pdf` cache for identical local requests, release binary strip/LTO profile, and product-facing proof/docs. Sole-Rust production remains the only engine path. Native optional packages are version-synced by the release script.

## 4.0.0

## 4.0.2

### Patch: repair linux-x64-gnu native tarball

- Emergency republish of all optional natives + main after `@sylphx/pdf-reader-mcp-linux-x64-gnu@4.0.1` registry metadata existed but tarball returned 404.
- Same sole-Rust dependency-closure content as 4.0.1 (`dependencies: {}`).
- Performance marketing remains withheld; goal complete remains unauthorized until full install proofs.

## 4.0.1

### Patch: production dependency closure + docs honesty

- Remove `pdfjs-dist`, `@modelcontextprotocol/sdk`, `pngjs`, and `zod` from **production** `dependencies` (moved to devDependencies for test/oracle only).
- Default `build` is launcher-only; `build:oracle-ts` is explicit test-only (`build:test`).
- Add `check:prod-dependency-closure` gate and wire into `prepublishOnly`.
- Restore real five-host proof standards (no deferred/cross-build pass).
- Rewrite README for users; keep migration internals out of the homepage.
- Performance marketing remains withheld until admissible same-host A/B evidence exists.
- Goal complete remains **false** until dependency-closure release + performance evidence + successor review.

### Major Changes

- **Freeze-lift authorized** by independent whole-product review (`review_pass_unfreeze_authorized_goal_incomplete`). Goal complete remains false until multi-channel publish/readback. **No marketing speedup claims.**
- **Sole-Rust production package (ADR-0006):** remove public `./typescript` export and stop shipping TypeScript PDF runtime / PDF.js workers in the npm artifact. Default entry is thin native launcher only. Historical TypeScript LKG remains external `@sylphx/pdf-reader-mcp@3.0.14`.
- **Publish freeze retained** until capability-first admission, same-host performance A/B, five real host proofs (incl. Darwin x64 on x86_64), and independent whole-product review authorize the exact candidate.
- **crates.io** explicitly not an admitted product channel unless re-admitted by ADR.
- Published **3.2.2 remains transitional** Rust-default with bundled TS rollback; do not market it as Sole Rust.

## 3.2.2

### Patch Changes

- Fix pure-Rust `serverInfo.description` still advertising experimental / not-published-npm-latest after 3.2.1. Instructions were already corrected; description now matches the published pure-Rust fail-closed default.

## 3.2.1

### Patch Changes

- Corrective release after 3.2.0 packaging audit: default `dist/runtime-entry.js` is **fail-closed** when the pure-Rust native optional package is missing (no automatic TypeScript fallback). TypeScript remains explicit rollback via `./typescript` or force flags. Fix Rust `serverInfo`/instructions that still advertised experimental/3.0.14-only production guidance. Harden verified-candidate admission (evidence authorization, zero `blockingForUnfreeze`, optional exact-HEAD match). Honest productTruth: four distinct host runtime proofs; Darwin x64 package published but host-unverified.

## 3.2.0

### Minor Changes

- Sole-runtime default: package bin/export prefer pure-Rust optional native MCP server via `dist/runtime-entry.js`, with TypeScript fallback (`./typescript`, force flags). Authorized after five-platform registry install + pure-Rust initialize proof for 3.1.4. Capability-first admission (ADR-0005); not exhaustive PDF.js output parity.

## 3.1.4

### Patch Changes

- Fix publish pipeline overwriting Ubuntu 22.04 linux-x64 native artifacts with the publish-job host `build:rust` binary (which reintroduced GLIBC_2.39). Restage matrix artifacts after host smoke and gate linux-x64 publish on GLIBC <= 2.35. TypeScript remains default; dropInFor3014=false.

## 3.1.3

### Patch Changes

- Rebuild Linux optional native packages on Ubuntu 22.04 so pure-Rust binaries do not require GLIBC_2.39. Keep TypeScript as default entry and dropInFor3014=false. Add five-platform registry install + pure-Rust initialize proof workflow.

## 3.1.2

### Patch Changes

- Stage B progress release after withdrawn 3.0.15–3.1.1 range: five-platform pure-Rust optional native packages with binary-gated publish, optionalDependencies wiring, and admission-gated multi-platform publish pipeline. TypeScript remains the default entry (`dropInFor3014=false`). Do not use withdrawn 3.0.15–3.1.1.

## 3.0.15

### Patch Changes

- [#554](https://github.com/SylphxAI/pdf-reader-mcp/pull/554) [`3391f59`](https://github.com/SylphxAI/pdf-reader-mcp/commit/3391f59b1635eb76688f5001d32698bb08accfcb) Thanks [@shtse8](https://github.com/shtse8)! - Prepare five-platform pure-Rust optional native packages for admission-gated publish while keeping TypeScript as the default entry. Wire optionalDependencies, binary-gated prepublish, multi-platform publish pipeline, and registry install proof harness. dropInFor3014 remains false.

## Recovery (2026-07-17, source only — no registry publish)

- Restore published product path on main to TypeScript **3.0.14** surface
  (`bin` → `dist/index.js`, `exports`, `files: [dist/…]`)
- Pure-Rust is opt-in (`PDF_READER_ENGINE_MODE=pure-rust`), not npm latest
- Fix pure-Rust `read_pdf` auto default when explicit `include_*` present
- Fix pure-Rust `search_pdf` snake_case offsets + honor `sources[].pages`
- Production contract tests target TypeScript path again
- Typed MCP inputSchema for pure-Rust tools
- Honest SSOT: docs/specs/pure-rust-capability-matrix.json
- True TS→Rust text differential (TypeScript oracle)
- **No npm/crates publish**

## Withdrawal (2026-07-17)

- **PUBLISH FREEZE.** No further registry publish until true capability parity + honest docs.
- **npm:** cannot unpublish (E405: package has dependents). Instead:
  - `npm deprecate` **`3.0.15`–`3.1.1`** with WITHDRAWN message
  - `dist-tag latest` restored to **`3.0.14`** (last known good TS path)
- **crates.io:** `3.1.1` of `pdf-reader-core` / `pdf-reader-mcp-server` / `pdf-reader-cli`
  cannot be deleted; **yank pending** (org token lacked yank scope — owner must
  `cargo yank --vers 3.1.1 <crate>` with a yank-capable token).
- GitHub releases `v3.0.15`–`v3.1.1` marked prerelease **WITHDRAWN**.

## 3.1.1 (unpublished)

### Patch Changes

- Withdrawn. Pure-Rust document twin field population was incomplete relative to
  3.0.14; marketing and key-presence tests oversold parity. Do not use.

## 3.1.0 (unpublished)

### Major Changes

- Withdrawn pure-Rust cutover. Incomplete capability surface vs 3.0.14.

## 3.1.1

### Patch Changes

- Complete pure-Rust Agent Document Twin capability surface + dual ecosystem install.

  - `read_pdf` now populates the full public field set: tables, trust_report,
    accessibility_report, document_ast, safety_findings, layout_diagnostics,
    outline/annotations/forms/attachments/structure/page geometry labels,
    OCR/visual provider placeholders with warnings
  - New capability-parity production contract
    (`test/production/capabilityParity.contract.test.ts`) fails if any public
    include\_\* field disappears
  - Pure-Rust benchmark harness (`bun run benchmark:pure-rust`) + industry write-up
    (`docs/performance/why-rust.md`)
  - crates.io packaging for `pdf-reader-core`, `pdf-reader-mcp-server`,
    `pdf-reader-cli` + GitHub Actions publish workflow
  - README dual install: npm MCP clients and cargo/crates.io Rust users

## 3.1.0

### Major Changes

- Pure-Rust production cutover. Remove the TypeScript parity bridge dual path.

  - Production process: `bin/pdf-reader-mcp` → `bin/native/pdf-reader-mcp-server` (rmcp only)
  - Tools: `read_pdf`, `search_pdf`, `pdf_evidence` (inspect) run entirely in Rust (`pdf-reader-core`)
  - URL loading with SSRF protection (IPv4 private + IPv6 transition addresses)
  - Visual ops (`render_page` / `extract_regions` / `ocr_pages` / `analyze_regions`) fail closed with guidance
  - Deleted `parity_bridge` and no longer require `dist/legacy-engine-runtime.js` for MCP

## 3.0.18

### Patch Changes

- Add star-project production-path public contract suite and fail-closed Zod validation.

  - New production contract tests exercise the published bin with full-parity engine
    (read_pdf option matrix, search_pdf, pdf_evidence ops, SSRF, fail-closed errors)
  - Wire `check:production-contract` into prepublish and release preflight
  - Enforce full Zod schema validation (including path/url exclusivity refinements)
    at both the TS MCP adapter and the parity-bridge engine boundary

## 3.0.17

### Patch Changes

- Converge production to a single Rust MCP process with full TypeScript tool parity.

  - Default process: Rust rmcp (`bin/pdf-reader-mcp` → `bin/native/pdf-reader-mcp-server`)
  - Default tool engine: full TypeScript V3 handlers via parity bridge
    (`dist/legacy-engine-runtime.js`) — drop-in with 3.0.14/3.0.16 behavior
  - Pure-Rust subset remains opt-in only: `PDF_READER_ENGINE_MODE=pure-rust`
  - Stage pdfjs worker into `dist/` for reliable engine runtime packaging
  - Add `docs/specs/rust-dropin-parity-matrix.json` as the pure-Rust completion SSOT

  This removes the dual production default (TS vs incomplete Rust) while keeping
  full capability during pure-Rust migration.

## 3.0.16

### Patch Changes

- Restore full TypeScript MCP as production default (3.0.14 drop-in).

  3.0.15 prematurely published an incomplete Rust-only MCP path that rejected
  remote URLs and much of the Agent Document Twin / pdf_evidence surface.
  Production default is restored to `dist/index.js` (full TS tools). Rust remains
  opt-in only via `PDF_READER_MCP_ENGINE=rust` until pure-Rust capability parity
  is proven. Includes the GHSA-f3xw-ff5r-rj7c SSRF guard fix on the TS path.

## 3.0.15

### Patch Changes

- [`df4aa80`](https://github.com/SylphxAI/pdf-reader-mcp/commit/df4aa80c29d61dc1248e3801b5f78206e8719bb3) Thanks [@shtse8](https://github.com/shtse8)! - Fix SSRF guard bypass via IPv6 transition addresses (NAT64 / 6to4 / Teredo).

  `isPrivateIpv6` now expands IPv6 literals to hextets and blocks:

  - NAT64 well-known `64:ff9b::/96` (RFC 6052) when the embedded IPv4 is non-public
  - NAT64 local-use `64:ff9b:1::/48` (RFC 8215) entirely
  - 6to4 `2002::/16` (RFC 3056) when the embedded IPv4 is non-public
  - Teredo `2001:0::/32` (RFC 4380) entirely
  - documentation `2001:db8::/32` and discard-only `100::/64`

  Regression coverage for GHSA-f3xw-ff5r-rj7c (reported by tonghuaroot).

## 3.0.14

### Patch Changes

- [#381](https://github.com/SylphxAI/pdf-reader-mcp/pull/381) [`38018ed`](https://github.com/SylphxAI/pdf-reader-mcp/commit/38018ede8b107e9df3eaaf7b434004fde448c3bd) Thanks [@shtse8](https://github.com/shtse8)! - Add official MCP Registry metadata (`server.json`, `mcpName`) and automate registry publishing on GitHub releases.

## 3.0.13

### Patch Changes

- [#375](https://github.com/SylphxAI/pdf-reader-mcp/pull/375) [`31f9b57`](https://github.com/SylphxAI/pdf-reader-mcp/commit/31f9b5743b073c83c05ea5e2004ef1ffcba1296b) Thanks [@shtse8](https://github.com/shtse8)! - Fix PdfSession acquire/destroyAll race so in-flight loads never throw "entry missing", and add auto-read OCR session handoff regression coverage.

## 3.0.12

### Patch Changes

- [#373](https://github.com/SylphxAI/pdf-reader-mcp/pull/373) [`7678a1b`](https://github.com/SylphxAI/pdf-reader-mcp/commit/7678a1b1745ba71432d4d558a4ada1b59d9a2352) Thanks [@shtse8](https://github.com/shtse8)! - Harden read_pdf overhead optimizations: serialize concurrent PdfSession acquires, skip auto-read when callers only specify source pages, add default balanced auto-read benchmark coverage, and expand regression tests for page budgets and session document reuse.

## 3.0.11

### Patch Changes

- [#371](https://github.com/SylphxAI/pdf-reader-mcp/pull/371) [`45e2130`](https://github.com/SylphxAI/pdf-reader-mcp/commit/45e21304325c3ef9aac6d9b1bf6726e05da3ae75) Thanks [@shtse8](https://github.com/shtse8)! - Optimize default `read_pdf` overhead by reusing one parsed PDF per source per request, bounding balanced/fast auto-read page extraction to the inspection sample budget, and omitting redundant per-page text MCP content parts when markdown/chunks are already present in the JSON payload.

## 3.0.10

### Patch Changes

- [#357](https://github.com/SylphxAI/pdf-reader-mcp/pull/357) [`68c4ebd`](https://github.com/SylphxAI/pdf-reader-mcp/commit/68c4ebd7cee48cf42ffbeb919d0eaa054ad33e4a) Thanks [@shtse8](https://github.com/shtse8)! - Consolidate last remaining duplicate geometry helpers.

  - Remove local `roundRatio` from `documentModel.ts` and `ocr.ts` — now import from `utils/geometry.ts`
  - Remove local `mergeBoxes` from `documentModel.ts` — replaced with `mergeBoundingBoxes` from `utils/geometry.ts`
  - Remove local `mergeBoundingBoxes` from `search.ts` — now imports from `utils/geometry.ts`

  After this change there is exactly ONE definition of each geometry helper:

  - `roundRatio`: 1 (was 3)
  - `mergeBoundingBoxes`: 1 (was 3 counting `mergeBoxes`)
  - Zero local duplicates remain.

  377 tests pass. No behavior changes.

## 3.0.9

### Patch Changes

- [#355](https://github.com/SylphxAI/pdf-reader-mcp/pull/355) [`eb85fbb`](https://github.com/SylphxAI/pdf-reader-mcp/commit/eb85fbbf0e5e5e4833aa76dab28d898dcedad295) Thanks [@shtse8](https://github.com/shtse8)! - Add unit tests for extracted modules and shared utilities.

  New test files (29 tests):

  - `test/pdf/autoReadPolicy.test.ts` (16 tests): covers hasExplicitReadOptions,
    shouldUseAutoRead, buildAutoDetailOptions (fast/balanced/full presets),
    buildReadOptions defaults and overrides, constants
  - `test/utils/errorHandling.test.ts` (5 tests): covers safeErrorMessage for
    PdfError, generic Error, non-Error values, null, and no-logger case
  - `test/utils/geometry.test.ts` (8 tests): covers roundRatio edge cases,
    mergeBoundingBoxes union computation, undefined filtering, NaN filtering

  Also restored `export` on `hasExplicitReadOptions` and `buildAutoDetailOptions`
  in autoReadPolicy.ts — they were incorrectly made private during Phase 3
  dead-export cleanup but need to be testable.

## 3.0.8

### Patch Changes

- [#353](https://github.com/SylphxAI/pdf-reader-mcp/pull/353) [`5757fc1`](https://github.com/SylphxAI/pdf-reader-mcp/commit/5757fc1cea721e75f621d528debde4933821bdf1) Thanks [@shtse8](https://github.com/shtse8)! - Final SOTA polish: tsconfig strictness, JSON.parse type safety, logger simplification.

  **tsconfig:**

  - Added `noUnusedLocals` and `noUnusedParameters` — dead code is now a compile error

  **JSON.parse type safety:**

  - `ocr.ts`: `JSON.parse(stdout) as RawOcrOutput` → `JSON.parse(stdout) as unknown` then
    validate before use. Provider output is untrusted and must not be trusted to
    match internal types without runtime validation.
  - `regionAnalysis.ts`: same fix for `RawRegionAnalysisOutput`

  **Logger simplification (125 → 76 lines, -39%):**

  - Consolidated `logWithContext` + `logSimple` into single `emit()` method
  - Removed duplicated `console[level]` branching (was repeated in both methods)
  - Console methods resolved at call time (not module load) so test spies work
  - Same behavior: structured context still logged for error/warn levels

## 3.0.7

### Patch Changes

- [#350](https://github.com/SylphxAI/pdf-reader-mcp/pull/350) [`977306b`](https://github.com/SylphxAI/pdf-reader-mcp/commit/977306b2d811fbbcc768a9678d889adcf253c439) Thanks [@shtse8](https://github.com/shtse8)! - Refactor: extract readCoordinator and autoReadPolicy from fat-controller readPdf handler.

  **Before:** `readPdf.ts` was a 955-line fat controller that merged three concerns:
  pipeline orchestration, auto-read decision policy, and MCP response assembly.

  **After:** Three clean modules with clear separation of concerns:

  - `src/pdf/autoReadPolicy.ts` (214 lines) — domain logic: which flags are
    explicit, when auto-read triggers, what fast/balanced/full presets mean,
    how to build processing options from schema input.
  - `src/pdf/readCoordinator.ts` (529 lines) — domain orchestration: the full
    extraction stage graph (metadata → structure → geometry → page-content →
    OCR → tables → elements → markdown → chunks → trust report → accessibility
    report → document map). Pure domain logic, no MCP transport awareness.
  - `src/handlers/readPdf.ts` (238 lines) — thin transport handler: schema →
    auto-read decision → coordinator call → MCP response assembly. Matches the
    shape of all other 7 handlers.

  **No behavior changes.** All 348 tests pass.

- [#351](https://github.com/SylphxAI/pdf-reader-mcp/pull/351) [`33d289b`](https://github.com/SylphxAI/pdf-reader-mcp/commit/33d289bcee92e66f9c2be6de832ed0c1cb2da26c) Thanks [@shtse8](https://github.com/shtse8)! - Refactor: consolidate PdfSource type SSOT and remove dead exports.

  **PdfSource SSOT:**

  - Replaced hand-written `PdfSource` interface in `types/pdf/source.ts` with a
    re-export from the schema definition (`schemas/readPdf.ts`). Now there is
    exactly one definition: `pdfSourceSchema` → `InferOutput` → re-export.
  - Removed dead `ReadPdfOptions` interface (never imported by any module).
  - Eliminates the split-brain SSOT drift risk: if someone adds a field to
    `pdfSourceSchema`, the type automatically updates everywhere.

  **Dead export cleanup:**

  - Removed `export` keyword from 14 functions/constants that are only used
    within their own file (never imported externally). This reduces the public
    API surface and prevents accidental coupling.
  - Affected: `buildSemanticHint`, `contentItemToElement`,
    `extractTablesFromTextItems`, `readConfiguredRegionAnalysisProviderConfig`,
    `analyzeRegionCropWithHttpProvider`, 7 DEFAULT\_ constants,
    `buildAutoDetailOptions`, `hasExplicitReadOptions`.

  **No behavior changes.** All 348 tests pass.

## 3.0.6

### Patch Changes

- [#348](https://github.com/SylphxAI/pdf-reader-mcp/pull/348) [`f583c1c`](https://github.com/SylphxAI/pdf-reader-mcp/commit/f583c1c2c3129a0025e2c9ec812e85cec4d672d3) Thanks [@shtse8](https://github.com/shtse8)! - Refactor: extract shared utilities to eliminate code duplication and improve maintainability.

  **Extracted shared utilities:**

  - `src/utils/errorHandling.ts` — `safeErrorMessage()` helper eliminates the duplicated PdfError-vs-generic error pattern across all 6 handlers (was repeated ~41 times with slight variations)
  - `src/utils/geometry.ts` — `roundRatio()` and `mergeBoundingBoxes()` shared across 5 modules (tableExtractor, accessibilityReport, documentMap, textLayer, extractor) — was independently defined 8 times
  - `src/utils/pdfjs.ts` — `destroyLoadingTask()` and `execFileAsync()` shared across 6 modules (search, regions, inspector, renderer, readPdf, ocr, regionAnalysis) — was duplicated 7 times

  **Improvements:**

  - Exported `Logger` class from `utils/logger.ts` for type-safe dependency injection
  - Consistent error-message sanitization policy (SSS-02) enforced through one function instead of ad-hoc duplication
  - Geometry helpers now use `Number.isFinite` validation (more robust than the old undefined-only checks)
  - Import ordering auto-fixed by Biome across all touched files

  **No behavior changes.** All 348 tests pass. The refactoring is purely structural — same inputs produce same outputs.

## 3.0.5

### Patch Changes

- [#344](https://github.com/SylphxAI/pdf-reader-mcp/pull/344) [`7645ca0`](https://github.com/SylphxAI/pdf-reader-mcp/commit/7645ca017b6ffe64d32235c03babcc6961722717) Thanks [@shtse8](https://github.com/shtse8)! - Add GHCR Docker publish workflow, fix CONTRIBUTING.md staleness, fix copyright consistency.

  - New `.github/workflows/docker.yml` — builds and pushes Docker image to GitHub Container Registry on main push and version tags, so the `ghcr.io/sylphxai/pdf-reader-mcp` reference resolves to a real image
  - Rewrote `CONTRIBUTING.md` — fixed stale org name (sylphlab → SylphxAI), corrected tooling references (ESLint/Prettier → Biome), corrected commands (npm → bun), added development setup and release process guidance
  - Fixed VitePress footer copyright (2024 Sylphx → 2024-2026 SylphxAI)
  - Updated README Docker section to show both GHCR pre-built image and local build

## 3.0.4

### Patch Changes

- [#342](https://github.com/SylphxAI/pdf-reader-mcp/pull/342) [`71f61d3`](https://github.com/SylphxAI/pdf-reader-mcp/commit/71f61d3c7949d6c7ad6ca440331e818747ec6673) Thanks [@shtse8](https://github.com/shtse8)! - Add Docker support, fix license branding, and ship examples in npm tarball.

  - New `Dockerfile` for containerized MCP server deployment with pre-installed Tesseract OCR
  - New `.dockerignore` for clean build context
  - Added comprehensive Docker documentation to installation guide (build, stdio run, HTTP run, Claude Desktop integration, OCR preset)
  - Added Docker badge and quick start to README
  - Fixed LICENSE copyright from "SylphLab" to "SylphxAI" (2024-2026)
  - Added `examples/` to `package.json` files field so examples ship in the npm tarball

## 3.0.3

### Patch Changes

- [#340](https://github.com/SylphxAI/pdf-reader-mcp/pull/340) [`1372688`](https://github.com/SylphxAI/pdf-reader-mcp/commit/1372688063e2b4e700b3a0ad403cc730dad556f4) Thanks [@shtse8](https://github.com/shtse8)! - Add social share OG image, evidence-first article, live docs site links in README and package.json.

  - New `docs/public/og-image.png` social share card (1200x630) with branded design featuring document-to-evidence visualization
  - New `docs/articles/evidence-first.md` — "Why Agents Need Evidence-First PDF Reading" article for SEO and content marketing
  - Updated `docs/.vitepress/config.ts` with `og:image` and `twitter:image` meta tags, article nav and sidebar entries
  - Updated `docs/index.md` homepage hero with article CTA button
  - Updated `README.md` docs table with live docs site link and article reference
  - Updated `package.json` homepage to point to live docs site for npm page discoverability

## 3.0.2

### Patch Changes

- [#337](https://github.com/SylphxAI/pdf-reader-mcp/pull/337) [`ae427bb`](https://github.com/SylphxAI/pdf-reader-mcp/commit/ae427bba1b918ba8661db20ce5d4fba040c325c5) Thanks [@shtse8](https://github.com/shtse8)! - Add GitHub Pages docs deployment workflow, examples directory with Agent Document Twin demo outputs and MCP client snippets, shareable benchmark proof page, and updated docs site navigation.

  - New `.github/workflows/docs.yml` deploys the VitePress docs site to GitHub Pages on every push to main.
  - New `examples/` directory with JSON request/response samples for all V3 tools (read_pdf, search_pdf, pdf_evidence) and MCP client installation snippets for Claude Code, Claude Desktop, Cursor, VS Code, Windsurf, Cline, Warp, and HTTP transport.
  - New `docs/benchmark.md` page with reproducible release evidence: 39/39 SOTA release gate checks, 69/69 quality checks, and performance benchmarks.
  - Updated VitePress config: benchmark page in nav and sidebar, corrected og:url and canonical for GitHub Pages.
  - Updated README documentation table with examples and benchmark links.

## 3.0.1

### Patch Changes

- [#332](https://github.com/SylphxAI/pdf-reader-mcp/pull/332) [`4c35f72`](https://github.com/SylphxAI/pdf-reader-mcp/commit/4c35f72862d729301e3912ab8c8317d2828cf4c2) Thanks [@shtse8](https://github.com/shtse8)! - Security: enforce HTTP transport authentication (X-API-Key).

  Previously the HTTP transport (`MCP_TRANSPORT=http`) read `MCP_API_KEY` and
  logged "API key authentication enabled" but never checked the header, so any
  client that could reach the port could call every PDF tool. The key is now
  enforced — when `MCP_API_KEY` is set, every `/mcp` request must present a
  matching `X-API-Key` header (constant-time comparison) or it is rejected with
  `401`; `/mcp/health` stays open. (CWE-306, reported by novice-22.)

  Hardening, both behavior changes for HTTP deployments:

  - `MCP_HTTP_HOST` now defaults to `127.0.0.1` (loopback) instead of `0.0.0.0`.
    Set it explicitly to expose other interfaces.
  - The server warns at startup when it binds a non-loopback host with no API key.

  `stdio` (the default transport) is unaffected.

## 3.0.0

### Major Changes

- [#330](https://github.com/SylphxAI/pdf-reader-mcp/pull/330) [`fa9b01d`](https://github.com/SylphxAI/pdf-reader-mcp/commit/fa9b01d33bd19cd095339336c585effbac66675d) Thanks [@shtse8](https://github.com/shtse8)! - Release PDF Reader MCP V3 with a smart, lower-context MCP tool surface.

  `read_pdf` is now the primary smart entrypoint: when no explicit `include_*`
  options are supplied, it automatically inspects each PDF, chooses a high-value
  extraction route, and returns routing metadata alongside the Agent Document
  Twin. Callers can still force manual extraction with `auto: false` or precise
  `include_*` options.

  The public MCP tool list is consolidated to `read_pdf`, `search_pdf`, and
  `pdf_evidence`. `pdf_evidence` replaces separate public inspect, render, crop,
  OCR, and visual-analysis tools with one operation-based evidence tool:
  `inspect`, `render_page`, `extract_regions`, `ocr_pages`, and
  `analyze_regions`.

  Public docs, API reference, guide, comparison, design notes, V3 spec, and the
  weekly update now describe the V3 smart-reader workflow and focused evidence
  operations.

## 2.7.2

### Patch Changes

- [#326](https://github.com/SylphxAI/pdf-reader-mcp/pull/326) [`1d5c5ea`](https://github.com/SylphxAI/pdf-reader-mcp/commit/1d5c5ea0f656db8fec595797fbd13c9eb38db15f) Thanks [@shtse8](https://github.com/shtse8)! - Rewrite the README as a shorter, higher-signal GitHub and npm landing page with clearer Agent Document Twin positioning, faster onboarding, evidence-backed capability labels, and stronger links into the full documentation.

## 2.7.1

### Patch Changes

- [#323](https://github.com/SylphxAI/pdf-reader-mcp/pull/323) [`8750e19`](https://github.com/SylphxAI/pdf-reader-mcp/commit/8750e198be24b52788fd9b2c67041dffb6bf9fde) Thanks [@shtse8](https://github.com/shtse8)! - Refresh public-facing positioning for the full-fidelity PDF intelligence release, including README, docs, comparison, release-proof labels, and npm metadata.

## 2.7.0

### Minor Changes

- [#314](https://github.com/SylphxAI/pdf-reader-mcp/pull/314) [`d3cae6b`](https://github.com/SylphxAI/pdf-reader-mcp/commit/d3cae6b58a65c472e495014d12b37388271ed517) Thanks [@shtse8](https://github.com/shtse8)! - Add table cell evidence coverage metrics, inferred-cell ratios, and incomplete
  geometry warnings so agents can route weak table evidence to visual
  verification. Add OCR-derived table extraction from normalized OCR word boxes
  for scanned pages. Add caption-derived visual enrichment candidates so
  `read_pdf` can route vector-drawn formulas, charts, figures, and diagrams to
  the configured visual-region provider even when the PDF does not expose image
  objects, and preserve those candidate regions in `read_pdf` and
  `document_map` when the optional provider is unavailable so agents can still
  crop or retry the same evidence regions. Add dedicated hidden-text trust
  routing for selectable text with zero
  or near-zero geometry. Replace the empty generated API reference with a
  maintained MCP API reference and remove unused TypeDoc docs tooling. Add
  direction-aware selectable-text ordering for right-to-left text runs and expose
  text-layer font, direction, transform, and end-of-line metadata coverage in the
  text layer and agent document map summaries. Add selected-page-scoped
  trust-report summary breakdowns for signal types, safety finding types, and
  page-risk counts, redacted trust-evidence snippets for common sensitive values,
  plus more specific routing guidance for overlapping, tiny, and off-page text.
  Add `trust_report_redaction` so callers can keep the default standard evidence
  redaction, opt into stricter phone/IP redaction, or explicitly preserve snippets
  for controlled local debugging while the selected policy is recorded in the
  trust report.
  Add accessibility report summary breakdowns for issue types, severity buckets,
  document-vs-page issue totals, page-grade buckets, and affected-page counts so
  agents can route tagged-PDF accessibility risks without scanning every raw
  issue first.
  Link accessibility report routing into the agent document map when both
  features are requested, including page report indexes, issue indexes, issue
  counts, affected-page routing arrays, and grade summaries without forcing raw
  structure-tree output.
  Link trust report routing into the agent document map when both features are
  requested, including page report indexes, signal indexes, risk, score, signal
  counts, high-signal routing, high/medium-risk routing arrays, and trust summary
  counts without forcing raw safety, layout, annotation, or table outputs.
  Add provider health metadata to optional OCR and visual-region provider status,
  including unavailable routing for built-in OCR presets when their executable is
  not installed. Extend the installed-provider benchmark so skipped providers
  still emit machine-readable certification profiles with skipped capabilities
  and safe provider-status metadata.
  Extend the deterministic quality benchmark JSON with a machine-readable SOTA
  final-bar coverage matrix that maps each capability area to benchmark scenarios
  and marks areas that still require installed-provider benchmark evidence.
  Extend the installed-provider benchmark JSON with a machine-readable final-bar
  provider evidence matrix that maps OCR and visual certification profiles to the
  capability areas they can certify when local providers are installed.
  Add shared benchmark artifact output support so performance, deterministic
  quality, and installed-provider reports can be written as profile-named JSON
  files for release evidence.
  Add a SOTA release gate over benchmark artifacts so release review fails until
  deterministic final-bar coverage, corpus evidence, and installed-provider
  final-bar evidence are complete.
  Add a package smoke gate and release preflight so the packed package must
  include the executable runtime artifact and matching package contract before
  publishing.
  Add provider benchmark quality metrics with thresholds, scores, expected
  evidence, and observed evidence for OCR and visual full-fidelity certification
  profiles.
  Link page-edge table continuation candidates when adjacent pages keep matching
  column geometry without repeating the header row.
  Add a deterministic reference visual-region provider for release certification
  fixtures and run the strict release-evidence gate in CI without publishing, so
  provider evidence regressions are caught before the release workflow.
  Make the release artifact script respect `MCP_PDF_BENCHMARK_OUTPUT_DIR` while
  preserving `benchmark-artifacts` as the default output directory.
  Add multi-caption and multi-target visual-layout fixture coverage for
  independent formula, chart, figure, and side-caption routing.
  Expand the installed-provider benchmark to score multiple runtime OCR fixtures
  and 10 visual-region certification fixtures across core and diverse visual
  profiles, with fixture-level expected and observed evidence in the provider
  quality report.
  Add a corpus benchmark artifact over checked-in and runtime-generated PDF
  archetypes, then require that corpus evidence in the SOTA release gate.
  Add an Ollama visual-region provider preset that sends crop images to the local
  `/api/generate` endpoint with JSON-only output and normalizes the returned
  evidence through the existing table/formula/chart/figure contract.
  Add an OpenAI-compatible visual-region provider preset that sends crop images as
  chat-completions `image_url` data URLs, supports optional bearer auth, and
  normalizes returned message content through the same evidence contract.
  Add LM Studio and llama.cpp visual-region provider presets that reuse the
  chat-completions crop data URL contract with local default endpoints, explicit
  model env vars, and deterministic benchmark coverage.
  Add external corpus manifest support to `benchmark:corpus` so teams can include
  operator-supplied real PDFs in the same corpus artifact without making release
  CI depend on bundled external files or network downloads.
  Add opt-in public URL support for external corpus manifests with required
  SHA256 validation, reusable cache paths, and artifact provenance for URL,
  checksum, and download/cache status. Private, loopback, and link-local URL
  hosts are blocked by default unless the existing private-IP development
  override is enabled.
  Add a checked-in public URL corpus manifest with official and publicly available
  PDF sources, pinned SHA256 values, source metadata, and package smoke coverage
  so users can reproduce real-world corpus artifacts without vendoring PDF bytes.
  The corpus benchmark now carries case-level capability tags and an
  artifact-level capability summary, and the package smoke gate verifies required
  public corpus capability coverage in the packed package. The SOTA release gate
  also verifies that corpus cases keep capability tags and that the corpus
  capability summary covers required release areas without failing tags.
  Add an opt-in public provider accuracy benchmark manifest and
  `benchmark:provider-manifest` script so configured visual-region providers can
  be scored against public PDF crops with pinned source metadata and checksums.
  The provider manifest artifact now carries capability tags and a
  capability-level summary so public proof can be reviewed by capability area,
  not only by aggregate score. The package smoke gate also verifies required
  public provider capability coverage in the packed package.
  Strengthen package smoke for public evidence manifests so published corpus
  cases must keep expected text/page/text-volume assertions and document-map/text
  layer read options, while published provider regions must keep positive-area
  bounding boxes, expected text terms, and normalized minimum confidence
  thresholds.
  Expand the checked-in public corpus and provider manifests with CDC statistical
  chart evidence plus public arXiv research-paper figure, formula, and table
  crops. The package smoke gate now also requires provider-region expected-kind
  coverage for chart, diagram, figure, formula, image, and table evidence so the
  published package cannot regress to a narrow visual manifest.
  Add `benchmark:provider-manifest-crops` so the same public provider manifest can
  first prove URL download, SHA256 validation, page rendering, declared crop
  geometry, crop byte evidence, render provenance, and capability summaries
  without requiring a visual-region provider or local model.
  The strict release artifact path now also writes a deterministic
  provider-manifest crop artifact over a local fixture manifest, and the SOTA
  release gate requires that artifact before publishing evidence can pass.
  Add deterministic provider-manifest scoring release evidence over local table,
  formula, chart, figure, and image regions. The SOTA release gate now requires
  that `pdf_provider_manifest_benchmark.json` prove visual-kind coverage,
  kind-specific assertions, crop provenance, and capability-summary coverage
  before publishing evidence can pass.

## 2.6.0

### Minor Changes

- [#310](https://github.com/SylphxAI/pdf-reader-mcp/pull/310) [`d368eb9`](https://github.com/SylphxAI/pdf-reader-mcp/commit/d368eb9b310f53b10ebc1abfc9447470f1518139) Thanks [@shtse8](https://github.com/shtse8)! - Add the v3 agent document map and visual evidence path for PDF intelligence workflows. `include_document_map` now returns linked pages, structured elements, citation chunks, layout diagnostics, safety findings, routing signals, page geometry, and summary counts while keeping image bytes out of JSON. This release batch also adds `include_text_layer`, a deterministic line and word text layer with page-level character ranges, best-effort bounding boxes, and provenance. It adds `include_document_ast`, a semantic tree with page, section, paragraph, list item, table, and image nodes linked back to element and chunk evidence. It adds `include_trust_report`, a consolidated local risk report for content safety, layout uncertainty, sparse/scanned pages, table quality warnings, and external-link routing. It adds `include_accessibility_report`, a deterministic accessibility report for tagged-PDF coverage, structure tree availability, heading roles, image alt-text verifiability, form labels, link labels, mark info, and accessibility permissions without claiming PDF/UA certification. It adds `search_pdf` for bounded literal search with snippets, match offsets, text-item bounding boxes, and provenance before heavier read/render/crop workflows. It adds `render_page`, which renders selected PDF pages as bounded PNG MCP image parts with JSON provenance, evidence IDs, pixel budgets, and page-level metadata for visual inspection and OCR routing. It adds `extract_regions` for PDF-coordinate bbox crops as focused PNG MCP image parts with crop metadata and provenance. It adds `analyze_regions`, an optional env-configured local visual-region provider pipeline that passes focused crops to a local command and normalizes table, chart, formula, figure, image-description, confidence, warning, crop evidence, and provenance fields without bundling a vision model. It adds `ocr_pages`, an optional env-configured local OCR provider pipeline that renders selected pages, passes temporary PNGs to a local command or `MCP_PDF_OCR_PRESET=tesseract`, and returns normalized OCR text, confidence, word boxes, language, render evidence IDs, and provenance without bundling an OCR model. `inspect_pdf` now reports safe optional-provider readiness for `ocr_pages` and `analyze_regions` without exposing local command paths or arguments. It also adds table quality diagnostics with inferred cell span/header hints, sparse-grid warnings, merged-cell candidate signals, and repeated-header continuation candidates. It also includes optional `include_layout_diagnostics` output with page layout profiles, reading-order confidence, column signals, and warnings for agent routing. The release batch updates `bun run benchmark` to run a reproducible local PDF intelligence benchmark with JSON output.

## 2.5.4

### Patch Changes

- [#308](https://github.com/SylphxAI/pdf-reader-mcp/pull/308) [`cabfdd6`](https://github.com/SylphxAI/pdf-reader-mcp/commit/cabfdd656cd1e8fe6548c9901b2e991cedd76c82) Thanks [@shtse8](https://github.com/shtse8)! - Add an `inspect_pdf` MCP tool that profiles PDFs before extraction, recommends `read_pdf` arguments, and flags likely OCR needs without adding heavy default dependencies.

## 2.5.3

### Patch Changes

- [#305](https://github.com/SylphxAI/pdf-reader-mcp/pull/305) [`f556e48`](https://github.com/SylphxAI/pdf-reader-mcp/commit/f556e486b69fcf89ff79bd7daf9273bbb71690a3) Thanks [@shtse8](https://github.com/shtse8)! - Use a scoped release bot token for automated Changesets release pull requests and publishing.

## 2.5.2

### Patch Changes

- [#303](https://github.com/SylphxAI/pdf-reader-mcp/pull/303) [`575f887`](https://github.com/SylphxAI/pdf-reader-mcp/commit/575f887ce5dbe27f2760f132ae20e2442fe73e73) Thanks [@shtse8](https://github.com/shtse8)! - Refresh CI coverage upload handling so release validation skips missing optional reports cleanly and uses the supported Codecov action path for test-result uploads.

## 2.5.1

### Patch Changes

- [#300](https://github.com/SylphxAI/pdf-reader-mcp/pull/300) [`8cdb556`](https://github.com/SylphxAI/pdf-reader-mcp/commit/8cdb556c9de1f25bcd24b00acb00cd5d3ad42113) Thanks [@shtse8](https://github.com/shtse8)! - Streamline release validation around Changesets, CI, and explicit package scripts for a clearer published package workflow.

## 2.5.0

### Minor Changes

- [#296](https://github.com/SylphxAI/pdf-reader-mcp/pull/296) [`3d6e015`](https://github.com/SylphxAI/pdf-reader-mcp/commit/3d6e015cbeb70e480af1f9e2cf9d2dd92ce8a55c) Thanks [@shtse8](https://github.com/shtse8)! - Add agent-ready PDF intelligence outputs with structured elements, semantic hints, Markdown and HTML renderers, citation chunks, richer table geometry, tagged structure trees, document signals, safety findings, quality evals, and public documentation.

## 2.4.3 (2026-05-30)

### 🐛 Bug Fixes

- require node >=22.13.0 to match pdfjs-dist v6 (#289) ([a0730b9](https://github.com/SylphxAI/pdf-reader-mcp/commit/a0730b9c4b63e6737cb2faa58dde612efa0e4344))

### 📚 Documentation

- **readme:** correct stale claims to match current stack (#288) ([941fcd8](https://github.com/SylphxAI/pdf-reader-mcp/commit/941fcd85d8b132a5e66e90eb58c99a236b459f01))

### 🔧 Chores

- **deps:** upgrade to latest, drop dead glob dep, pdfjs-dist v6 (#287) ([de2d340](https://github.com/SylphxAI/pdf-reader-mcp/commit/de2d3407ece5cae21a9ede62964e362686be592b))

## 2.4.2 (2026-05-22)

### 🐛 Bug Fixes

- **ci:** regenerate bun.lock after typescript 6 + bunup bumps ([1172f44](https://github.com/SylphxAI/pdf-reader-mcp/commit/1172f447641651a946a99c0fac888e86c6598fa2))

### 👷 CI

- fix self-hosted runner label and drop unused Vercel config (#284) ([f370b48](https://github.com/SylphxAI/pdf-reader-mcp/commit/f370b48ebf243c27590f1a643c30e244a4535bba))

### 🔧 Chores

- remove project-scoped AI settings (#283) ([3dc2d0c](https://github.com/SylphxAI/pdf-reader-mcp/commit/3dc2d0ca7d9ff60e7366f0353d1acfbf6296c4d7))

## 2.4.1 (2026-05-20)

### 🐛 Bug Fixes

- **security:** patch SSS-02/03/07/08 findings (closes #279) (#280) ([b77f9f5](https://github.com/SylphxAI/pdf-reader-mcp/commit/b77f9f57b8cd68d600f92382f738d30e4439244b))
- remove wildcard CORS default in HTTP transport mode (CWE-942) (#278) ([4e265dd](https://github.com/SylphxAI/pdf-reader-mcp/commit/4e265dd46bf847a445d7472b76d6d5896bd7ac1c))

### 🔧 Chores

- migrate sylphx.json to sylphx.toml (#281) ([82833db](https://github.com/SylphxAI/pdf-reader-mcp/commit/82833db9a6fb3a78cedb1c6062540a4046be80b3))

## 2.4.0 (2026-05-03)

### ✨ Features

- add filesystem and HTTP access restrictions (closes #274) ([78e26df](https://github.com/SylphxAI/pdf-reader-mcp/commit/78e26df032415df2caaca7ae8c39a38c3bcd92ed))

## 2.3.1 (2026-04-19)

### 🐛 Bug Fixes

- **test:** forward all fs/promises exports through mock to unblock CI release ([f594561](https://github.com/SylphxAI/pdf-reader-mcp/commit/f594561530489018479800cbebc2390151443b49))
- provide all pdfjs-dist resource URLs to restore image decoding (closes #271) ([42baf54](https://github.com/SylphxAI/pdf-reader-mcp/commit/42baf545781903cb4cc2114fd304e924eaaba4c2))

### 👷 CI

- migrate to flat runner labels [self-hosted, sylphx, {platform}, {size}] ([e8d3edd](https://github.com/SylphxAI/pdf-reader-mcp/commit/e8d3edddd02e2a167a12c4aae9e053f5b9b054a9))
- migrate runs-on to ARC v2 runner labels ([d5bc801](https://github.com/SylphxAI/pdf-reader-mcp/commit/d5bc801c930bb3f0215aa4737c03c6f4599c272b))
- migrate all GitHub workflows to use self-hosted runner (#264) ([cfe334a](https://github.com/SylphxAI/pdf-reader-mcp/commit/cfe334acd767fb011646271b7fbea19637051670))

### 🔧 Chores

- **deps:** update deps and override vulnerable transitive packages ([c472ee7](https://github.com/SylphxAI/pdf-reader-mcp/commit/c472ee73d319c40db0ffe822f7ce9ba0c1ce4888))
- add sylphx.json for Platform build config ([b393d46](https://github.com/SylphxAI/pdf-reader-mcp/commit/b393d46b6e40020e0b39ccc82701e85df114d337))

## 2.3.0 (2026-02-04)

### ✨ Features

- add table extraction support (closes #259) ([3f72992](https://github.com/SylphxAI/pdf-reader-mcp/commit/3f72992cce9c4ea3f491ccaae79a6216d2318cf5))

### 🐛 Bug Fixes

- **deps:** update glob to fix brace-expansion vulnerability ([945e66c](https://github.com/SylphxAI/pdf-reader-mcp/commit/945e66c91916ed6d66e5642e50f125a0fb0f1d33))

## 2.2.0 (2026-01-28)

### ✨ Features

- add HTTP transport for remote access (closes #255) ([13c1342](https://github.com/SylphxAI/pdf-reader-mcp/commit/13c134287c7004d93b04df1c18b66eff49013a83))
- **docs:** migrate to VitePress with modern sleek design ([ed1d152](https://github.com/SylphxAI/pdf-reader-mcp/commit/ed1d1527355821d109667d6d9b796db8e1a4cd1e))

### 🐛 Bug Fixes

- resolve release workflow issues ([33431ca](https://github.com/SylphxAI/pdf-reader-mcp/commit/33431ca678ea815783476ace658e04d751657f3c))
- prevent UI blocking with large PDFs (closes #254) ([812ba51](https://github.com/SylphxAI/pdf-reader-mcp/commit/812ba512fd32d03cd131b8f70a4b5182501c44e0))
- **vercel:** use npx for vitepress build command ([7b62920](https://github.com/SylphxAI/pdf-reader-mcp/commit/7b629209133850d2058695e55913b01ef93d6842))
- **docs:** rebuild docs with proper leaf configuration ([8ebeca1](https://github.com/SylphxAI/pdf-reader-mcp/commit/8ebeca1123f0ef90fcc08f25e746eb69bb7da8f7))
- **docs:** commit pre-built docs for Vercel deployment ([18874f7](https://github.com/SylphxAI/pdf-reader-mcp/commit/18874f7147945c4cdf07f28009f62a23c8ef62fc))

### 🔧 Chores

- format config files and rebuild dist ([7d95c56](https://github.com/SylphxAI/pdf-reader-mcp/commit/7d95c56d6e34069e09d5df8349348fa2445268b9))
- update docs URL to pdf-reader-mcp.sylphx.com ([0c131c0](https://github.com/SylphxAI/pdf-reader-mcp/commit/0c131c08e3a2477281af904eef29cce05114f655))

## 2.1.0 (2025-12-17)

### ✨ Features

- add CMap support for Japanese/CJK PDF text extraction (#251) ([8ba4453](https://github.com/SylphxAI/pdf-reader-mcp/commit/8ba4453282e1583e9dfc003f731f32dff98da86e))

## 2.0.8 (2025-12-05)

### 🐛 Bug Fixes

- **build:** rebuild dist for Vex migration ([ab5d501](https://github.com/SylphxAI/pdf-reader-mcp/commit/ab5d501a1dd1a1c6a5281bf06a1e645d1bb6e47e))

### ♻️ Refactoring

- **schema:** migrate from Zod to Vex ([efc2dce](https://github.com/SylphxAI/pdf-reader-mcp/commit/efc2dce4c57512c442d1e4185e7bb4234406ce82))

### 🔧 Chores

- **deps:** upgrade @sylphx/mcp-server-sdk to ^2.1.0 ([64b6381](https://github.com/SylphxAI/pdf-reader-mcp/commit/64b63815adbcbbe80e6bc5a302ab42ff90b0fdc1))

## 2.0.7 (2025-12-03)

### 🐛 Bug Fixes

- remove types export for CLI tool ([e222734](https://github.com/SylphxAI/pdf-reader-mcp/commit/e2227348d80d39ddf94fcf19df5595d20a67446d))
- use local doctor in lefthook ([c59e9cb](https://github.com/SylphxAI/pdf-reader-mcp/commit/c59e9cbf06039fb104719376e487433f0b80c877))
- update mcp-server-sdk to 1.3.0 ([817a7a2](https://github.com/SylphxAI/pdf-reader-mcp/commit/817a7a2f295abffc2d7737a70a2da43dd12a0862))
- use bunx for leaf commands in scripts ([1ef81fd](https://github.com/SylphxAI/pdf-reader-mcp/commit/1ef81fdcf5ec87ef449aa1db9ee5c5a99fc4e75e))
- update vercel config for leaf docs ([5f57838](https://github.com/SylphxAI/pdf-reader-mcp/commit/5f57838075b6712012fad2b2170685bc32a10237))

### 📚 Documentation

- overhaul documentation ([4a89f85](https://github.com/SylphxAI/pdf-reader-mcp/commit/4a89f85e8b843b93bfc538c2964b86133f4ab5d3))

### 🔧 Chores

- trigger release PR ([d0d1a2e](https://github.com/SylphxAI/pdf-reader-mcp/commit/d0d1a2e70cf76327ee6f5329099469d3567ee2b1))
- test bump action fix ([fa4995a](https://github.com/SylphxAI/pdf-reader-mcp/commit/fa4995ae46ae02aa3ce4aee2265de1608eeaf2e8))
- update doctor and lefthook ([07c5f44](https://github.com/SylphxAI/pdf-reader-mcp/commit/07c5f44aef014d05bb9bdfd63a3319c300f7d383))
- trigger release workflow ([f00660f](https://github.com/SylphxAI/pdf-reader-mcp/commit/f00660f1256dab6c0bfac8dc2eb21d71ea5aa36a))
- update dependencies and fix doctor issues ([e3fc487](https://github.com/SylphxAI/pdf-reader-mcp/commit/e3fc4872ff35dd1083c65d37005b5b9224518e74))
- update @sylphx/doctor to 1.26.0 ([8082da0](https://github.com/SylphxAI/pdf-reader-mcp/commit/8082da055bc0bbb862bc9513f45ab9d44aa7ad4a))
- migrate biome config to 2.3.8 ([1318b94](https://github.com/SylphxAI/pdf-reader-mcp/commit/1318b94aa8ab78a90a6bf29b703e458f9fcb60f6))

## 2.0.6 (2025-12-03)

### 🐛 Bug Fixes

- use local doctor in lefthook ([c59e9cb](https://github.com/SylphxAI/pdf-reader-mcp/commit/c59e9cbf06039fb104719376e487433f0b80c877))
- update mcp-server-sdk to 1.3.0 ([817a7a2](https://github.com/SylphxAI/pdf-reader-mcp/commit/817a7a2f295abffc2d7737a70a2da43dd12a0862))
- use bunx for leaf commands in scripts ([1ef81fd](https://github.com/SylphxAI/pdf-reader-mcp/commit/1ef81fdcf5ec87ef449aa1db9ee5c5a99fc4e75e))
- update vercel config for leaf docs ([5f57838](https://github.com/SylphxAI/pdf-reader-mcp/commit/5f57838075b6712012fad2b2170685bc32a10237))

### 📚 Documentation

- overhaul documentation ([4a89f85](https://github.com/SylphxAI/pdf-reader-mcp/commit/4a89f85e8b843b93bfc538c2964b86133f4ab5d3))

### 🔧 Chores

- trigger release PR ([d0d1a2e](https://github.com/SylphxAI/pdf-reader-mcp/commit/d0d1a2e70cf76327ee6f5329099469d3567ee2b1))
- test bump action fix ([fa4995a](https://github.com/SylphxAI/pdf-reader-mcp/commit/fa4995ae46ae02aa3ce4aee2265de1608eeaf2e8))
- update doctor and lefthook ([07c5f44](https://github.com/SylphxAI/pdf-reader-mcp/commit/07c5f44aef014d05bb9bdfd63a3319c300f7d383))
- trigger release workflow ([f00660f](https://github.com/SylphxAI/pdf-reader-mcp/commit/f00660f1256dab6c0bfac8dc2eb21d71ea5aa36a))
- update dependencies and fix doctor issues ([e3fc487](https://github.com/SylphxAI/pdf-reader-mcp/commit/e3fc4872ff35dd1083c65d37005b5b9224518e74))
- update @sylphx/doctor to 1.26.0 ([8082da0](https://github.com/SylphxAI/pdf-reader-mcp/commit/8082da055bc0bbb862bc9513f45ab9d44aa7ad4a))
- migrate biome config to 2.3.8 ([1318b94](https://github.com/SylphxAI/pdf-reader-mcp/commit/1318b94aa8ab78a90a6bf29b703e458f9fcb60f6))

## 2.0.5 (2025-12-03)

### 🐛 Bug Fixes

- use local doctor in lefthook ([c59e9cb](https://github.com/SylphxAI/pdf-reader-mcp/commit/c59e9cbf06039fb104719376e487433f0b80c877))
- update mcp-server-sdk to 1.3.0 ([817a7a2](https://github.com/SylphxAI/pdf-reader-mcp/commit/817a7a2f295abffc2d7737a70a2da43dd12a0862))
- use bunx for leaf commands in scripts ([1ef81fd](https://github.com/SylphxAI/pdf-reader-mcp/commit/1ef81fdcf5ec87ef449aa1db9ee5c5a99fc4e75e))
- update vercel config for leaf docs ([5f57838](https://github.com/SylphxAI/pdf-reader-mcp/commit/5f57838075b6712012fad2b2170685bc32a10237))

### 📚 Documentation

- overhaul documentation ([4a89f85](https://github.com/SylphxAI/pdf-reader-mcp/commit/4a89f85e8b843b93bfc538c2964b86133f4ab5d3))

### 🔧 Chores

- test bump action fix ([fa4995a](https://github.com/SylphxAI/pdf-reader-mcp/commit/fa4995ae46ae02aa3ce4aee2265de1608eeaf2e8))
- update doctor and lefthook ([07c5f44](https://github.com/SylphxAI/pdf-reader-mcp/commit/07c5f44aef014d05bb9bdfd63a3319c300f7d383))
- trigger release workflow ([f00660f](https://github.com/SylphxAI/pdf-reader-mcp/commit/f00660f1256dab6c0bfac8dc2eb21d71ea5aa36a))
- update dependencies and fix doctor issues ([e3fc487](https://github.com/SylphxAI/pdf-reader-mcp/commit/e3fc4872ff35dd1083c65d37005b5b9224518e74))
- update @sylphx/doctor to 1.26.0 ([8082da0](https://github.com/SylphxAI/pdf-reader-mcp/commit/8082da055bc0bbb862bc9513f45ab9d44aa7ad4a))
- migrate biome config to 2.3.8 ([1318b94](https://github.com/SylphxAI/pdf-reader-mcp/commit/1318b94aa8ab78a90a6bf29b703e458f9fcb60f6))

## 2.0.3 (2025-11-30)

### 🐛 Bug Fixes

- remove unnecessary path access restrictions ([9615b2d](https://github.com/SylphxAI/pdf-reader-mcp/commit/9615b2d6f2517b44d64bbeaded6f614e1533a4c7))

### 🔧 Chores

- update lockfile for glob 13.0.0 ([4a26173](https://github.com/SylphxAI/pdf-reader-mcp/commit/4a261738c758dc0048fa421c5491e86f64971c81))
- **deps:** bump glob from 11.1.0 to 13.0.0 (#225) ([a19cfac](https://github.com/SylphxAI/pdf-reader-mcp/commit/a19cface62597b572846bdde8353f04c108869f9))

## 2.0.2 (2025-11-27)

### 🐛 Bug Fixes

- upgrade mcp-server-sdk to 1.2.0 ([32bda52](https://github.com/SylphxAI/pdf-reader-mcp/commit/32bda52228bfbcafdb9bcfee6450ccb3deab9afb))

## 2.0.1 (2025-11-27)

### 🐛 Bug Fixes

- ensure mcp-server-sdk 1.1.2 with correct tools/list response ([db65572](https://github.com/SylphxAI/pdf-reader-mcp/commit/db6557209adb85497223a043814963e59f68b06c))
- upgrade mcp-server-sdk to 2.0.0 to fix tools/list response ([ebd211f](https://github.com/SylphxAI/pdf-reader-mcp/commit/ebd211fe44fd364ddd92d8820103404e57992513))
- upgrade mcp-server-sdk to 1.1.2 ([80cc8c5](https://github.com/SylphxAI/pdf-reader-mcp/commit/80cc8c57d48da40f06e6e02a12718bd23bd1a736))

## 2.0.0 (2025-11-27)

### 🐛 Bug Fixes

- upgrade SDK to 1.1.1 with Node.js support ([26bb70d](https://github.com/SylphxAI/pdf-reader-mcp/commit/26bb70d310df4f82bf69a46fc396f585a4ead621))
- 💥 use bun shebang for proper runtime support ([00a07fd](https://github.com/SylphxAI/pdf-reader-mcp/commit/00a07fdeec4836443b9242ed9f663616ae448b24))

### 💥 Breaking Changes

- use bun shebang for proper runtime support ([00a07fd](https://github.com/SylphxAI/pdf-reader-mcp/commit/00a07fdeec4836443b9242ed9f663616ae448b24))
  Requires Bun runtime instead of Node.js

## 1.4.0 (2025-11-27)

### ✨ Features

- migrate documentation from VitePress to Leaf ([dd1d9ee](https://github.com/SylphxAI/pdf-reader-mcp/commit/dd1d9ee9a3250a3de9f9e297535c3bbe8a8f6527))

### 🐛 Bug Fixes

- **ci:** use explicit path for lefthook in prepare script ([40c3655](https://github.com/SylphxAI/pdf-reader-mcp/commit/40c36554a8958ded046c54fbfaad208b8fbad719))
- **security:** override js-yaml to fix vulnerability ([ce7acc8](https://github.com/SylphxAI/pdf-reader-mcp/commit/ce7acc808b2c174eea03c4ecc3de3699994d8133))
- **ci:** allow bun install without frozen-lockfile for Dependabot PRs ([af10706](https://github.com/SylphxAI/pdf-reader-mcp/commit/af107067d7dcb1851c82d97c6a6896275985e263))
- upgrade to SDK 1.0.0 and Zod 4 for proper JSON Schema support ([e9e21d5](https://github.com/SylphxAI/pdf-reader-mcp/commit/e9e21d57edcc2f3ec7e9c96fd9d6e5c062ab1fd0))
- improve image extraction timeout handling ([c9e6f55](https://github.com/SylphxAI/pdf-reader-mcp/commit/c9e6f55c90230f2eb2ccc8148470b130bf80f9c1))
- critical security and performance improvements ([19c7451](https://github.com/SylphxAI/pdf-reader-mcp/commit/19c74518fd4f39f2115a0aef9d64733bb26f60df))

### ♻️ Refactoring

- migrate from @modelcontextprotocol/sdk to @sylphx/mcp-server-sdk ([98efbbb](https://github.com/SylphxAI/pdf-reader-mcp/commit/98efbbb1a304b6aa9e30dead35f0fa6379939546))
- add structured logging system ([a337d93](https://github.com/SylphxAI/pdf-reader-mcp/commit/a337d93c35abe16b102632a3e9871a6f3a94bdc1))
- deduplicate image extraction logic ([2e6ef33](https://github.com/SylphxAI/pdf-reader-mcp/commit/2e6ef33577b7dbf902f88d4ecd4f33e2d1386b89))
- implement proper PDF document resource cleanup ([7893cf6](https://github.com/SylphxAI/pdf-reader-mcp/commit/7893cf63b07f0013b4f89a7dab91df4e7a1988c3))

### 📚 Documentation

- add installation guides for VS Code, Claude Code, Cursor, Windsurf, Cline, Warp ([28a3bf1](https://github.com/SylphxAI/pdf-reader-mcp/commit/28a3bf1ae0d02abfedbbd9e371952a974c3aae08))

### 🔧 Chores

- upgrade @sylphx/bump to v0.12.1 ([9c597fb](https://github.com/SylphxAI/pdf-reader-mcp/commit/9c597fbd052fe2171760229a46f4e49550a7aecb))
- upgrade @sylphx/doctor to v1.23.3 and @sylphx/bump to v0.10.2 ([ff6849e](https://github.com/SylphxAI/pdf-reader-mcp/commit/ff6849e7a49596da449baa7b5e14f9ecaeedf4af))
- upgrade @sylphx/doctor to v1.23.2 ([9ab92cf](https://github.com/SylphxAI/pdf-reader-mcp/commit/9ab92cf15e43aed336c771140d2675aa1c96ef65))
- migrate tooling to @sylphx ecosystem ([fc2471f](https://github.com/SylphxAI/pdf-reader-mcp/commit/fc2471ff61dcac287ec6d27f7038fdaaa088a727))
- upgrade all packages to latest versions ([8b6730b](https://github.com/SylphxAI/pdf-reader-mcp/commit/8b6730bd86fcb8d992200574bce66946bec00886))
- cleanup unused files and folders ([8834d09](https://github.com/SylphxAI/pdf-reader-mcp/commit/8834d09e1000ff57bae530a5ed069cc3b50a7866))
- migrate from Vitest to Bun test runner ([7382d1b](https://github.com/SylphxAI/pdf-reader-mcp/commit/7382d1b037805d0f47271676d71bd65721f50d8e))
- adjust coverage thresholds after adding defensive code ([3780190](https://github.com/SylphxAI/pdf-reader-mcp/commit/3780190625d2b5a04a3f3d9a42f17998132de672))

## 1.5.0 (2025-11-27)

### ✨ Features

- migrate documentation from VitePress to Leaf ([dd1d9ee](https://github.com/SylphxAI/pdf-reader-mcp/commit/dd1d9ee9a3250a3de9f9e297535c3bbe8a8f6527))

### 🐛 Bug Fixes

- **security:** override js-yaml to fix vulnerability ([ce7acc8](https://github.com/SylphxAI/pdf-reader-mcp/commit/ce7acc808b2c174eea03c4ecc3de3699994d8133))
- **ci:** allow bun install without frozen-lockfile for Dependabot PRs ([af10706](https://github.com/SylphxAI/pdf-reader-mcp/commit/af107067d7dcb1851c82d97c6a6896275985e263))
- upgrade to SDK 1.0.0 and Zod 4 for proper JSON Schema support ([e9e21d5](https://github.com/SylphxAI/pdf-reader-mcp/commit/e9e21d57edcc2f3ec7e9c96fd9d6e5c062ab1fd0))
- improve image extraction timeout handling ([c9e6f55](https://github.com/SylphxAI/pdf-reader-mcp/commit/c9e6f55c90230f2eb2ccc8148470b130bf80f9c1))
- critical security and performance improvements ([19c7451](https://github.com/SylphxAI/pdf-reader-mcp/commit/19c74518fd4f39f2115a0aef9d64733bb26f60df))

### ♻️ Refactoring

- migrate from @modelcontextprotocol/sdk to @sylphx/mcp-server-sdk ([98efbbb](https://github.com/SylphxAI/pdf-reader-mcp/commit/98efbbb1a304b6aa9e30dead35f0fa6379939546))
- add structured logging system ([a337d93](https://github.com/SylphxAI/pdf-reader-mcp/commit/a337d93c35abe16b102632a3e9871a6f3a94bdc1))
- deduplicate image extraction logic ([2e6ef33](https://github.com/SylphxAI/pdf-reader-mcp/commit/2e6ef33577b7dbf902f88d4ecd4f33e2d1386b89))
- implement proper PDF document resource cleanup ([7893cf6](https://github.com/SylphxAI/pdf-reader-mcp/commit/7893cf63b07f0013b4f89a7dab91df4e7a1988c3))

### 📚 Documentation

- add installation guides for VS Code, Claude Code, Cursor, Windsurf, Cline, Warp ([28a3bf1](https://github.com/SylphxAI/pdf-reader-mcp/commit/28a3bf1ae0d02abfedbbd9e371952a974c3aae08))

### 🔧 Chores

- **release:** @sylphx/pdf-reader-mcp@1.4.0 (#227) ([b3c1a58](https://github.com/SylphxAI/pdf-reader-mcp/commit/b3c1a583ca40d4ad1962b822fb36e9d2b842223e))
- upgrade @sylphx/doctor to v1.23.3 and @sylphx/bump to v0.10.2 ([ff6849e](https://github.com/SylphxAI/pdf-reader-mcp/commit/ff6849e7a49596da449baa7b5e14f9ecaeedf4af))
- upgrade @sylphx/doctor to v1.23.2 ([9ab92cf](https://github.com/SylphxAI/pdf-reader-mcp/commit/9ab92cf15e43aed336c771140d2675aa1c96ef65))
- migrate tooling to @sylphx ecosystem ([fc2471f](https://github.com/SylphxAI/pdf-reader-mcp/commit/fc2471ff61dcac287ec6d27f7038fdaaa088a727))
- upgrade all packages to latest versions ([8b6730b](https://github.com/SylphxAI/pdf-reader-mcp/commit/8b6730bd86fcb8d992200574bce66946bec00886))
- cleanup unused files and folders ([8834d09](https://github.com/SylphxAI/pdf-reader-mcp/commit/8834d09e1000ff57bae530a5ed069cc3b50a7866))
- migrate from Vitest to Bun test runner ([7382d1b](https://github.com/SylphxAI/pdf-reader-mcp/commit/7382d1b037805d0f47271676d71bd65721f50d8e))
- adjust coverage thresholds after adding defensive code ([3780190](https://github.com/SylphxAI/pdf-reader-mcp/commit/3780190625d2b5a04a3f3d9a42f17998132de672))

## 1.4.0 (2025-11-27)

### ✨ Features

- migrate documentation from VitePress to Leaf ([dd1d9ee](https://github.com/SylphxAI/pdf-reader-mcp/commit/dd1d9ee9a3250a3de9f9e297535c3bbe8a8f6527))

### 🐛 Bug Fixes

- **ci:** allow bun install without frozen-lockfile for Dependabot PRs ([af10706](https://github.com/SylphxAI/pdf-reader-mcp/commit/af107067d7dcb1851c82d97c6a6896275985e263))
- upgrade to SDK 1.0.0 and Zod 4 for proper JSON Schema support ([e9e21d5](https://github.com/SylphxAI/pdf-reader-mcp/commit/e9e21d57edcc2f3ec7e9c96fd9d6e5c062ab1fd0))
- improve image extraction timeout handling ([c9e6f55](https://github.com/SylphxAI/pdf-reader-mcp/commit/c9e6f55c90230f2eb2ccc8148470b130bf80f9c1))
- critical security and performance improvements ([19c7451](https://github.com/SylphxAI/pdf-reader-mcp/commit/19c74518fd4f39f2115a0aef9d64733bb26f60df))

### ♻️ Refactoring

- migrate from @modelcontextprotocol/sdk to @sylphx/mcp-server-sdk ([98efbbb](https://github.com/SylphxAI/pdf-reader-mcp/commit/98efbbb1a304b6aa9e30dead35f0fa6379939546))
- add structured logging system ([a337d93](https://github.com/SylphxAI/pdf-reader-mcp/commit/a337d93c35abe16b102632a3e9871a6f3a94bdc1))
- deduplicate image extraction logic ([2e6ef33](https://github.com/SylphxAI/pdf-reader-mcp/commit/2e6ef33577b7dbf902f88d4ecd4f33e2d1386b89))
- implement proper PDF document resource cleanup ([7893cf6](https://github.com/SylphxAI/pdf-reader-mcp/commit/7893cf63b07f0013b4f89a7dab91df4e7a1988c3))

### 🔧 Chores

- upgrade @sylphx/doctor to v1.23.3 and @sylphx/bump to v0.10.2 ([ff6849e](https://github.com/SylphxAI/pdf-reader-mcp/commit/ff6849e7a49596da449baa7b5e14f9ecaeedf4af))
- upgrade @sylphx/doctor to v1.23.2 ([9ab92cf](https://github.com/SylphxAI/pdf-reader-mcp/commit/9ab92cf15e43aed336c771140d2675aa1c96ef65))
- migrate tooling to @sylphx ecosystem ([fc2471f](https://github.com/SylphxAI/pdf-reader-mcp/commit/fc2471ff61dcac287ec6d27f7038fdaaa088a727))
- upgrade all packages to latest versions ([8b6730b](https://github.com/SylphxAI/pdf-reader-mcp/commit/8b6730bd86fcb8d992200574bce66946bec00886))
- cleanup unused files and folders ([8834d09](https://github.com/SylphxAI/pdf-reader-mcp/commit/8834d09e1000ff57bae530a5ed069cc3b50a7866))
- migrate from Vitest to Bun test runner ([7382d1b](https://github.com/SylphxAI/pdf-reader-mcp/commit/7382d1b037805d0f47271676d71bd65721f50d8e))
- adjust coverage thresholds after adding defensive code ([3780190](https://github.com/SylphxAI/pdf-reader-mcp/commit/3780190625d2b5a04a3f3d9a42f17998132de672))

## 1.3.2

### Patch Changes

- c97a5c0: Refactor CI workflows to use company release standard. Simplified CI workflow for validation only and enhanced release workflow with full configuration.

## 1.3.1

### Patch Changes

- b19fdaa: Refactor CI workflows to use company standard release flow and improve separation of concerns

All notable changes to this project will be documented in this file. Releases are managed with [Changesets](https://github.com/changesets/changesets).

## [1.3.0](https://github.com/SylphxAI/pdf-reader-mcp/compare/v1.2.0...v1.3.0) (2025-11-06)

### Features

- **Path Handling**: Remove absolute path restriction ([#212](https://github.com/SylphxAI/pdf-reader-mcp/pull/212))
  - **BREAKING CHANGE**: Absolute paths are now supported for local PDF files
  - Both absolute and relative paths are accepted in the `path` parameter
  - Relative paths are resolved against the current working directory (process.cwd())
  - Fixes [#136](https://github.com/SylphxAI/pdf-reader-mcp/issues/136) - MCP error -32602: Absolute paths are not allowed
  - Windows paths (e.g., `C:\Users\...`) and Unix paths (e.g., `/home/...`) now work correctly
  - Configure working directory via `cwd` in MCP server settings for relative path resolution

### Bug Fixes

- Fix Zod validation error handling - use `error.issues` instead of `error.errors`
- Update dependencies to latest versions (Zod 3.25.76, @modelcontextprotocol/sdk 1.21.0)

### Code Quality

- All 103 tests passing
- Coverage: 94%+ lines, 98%+ functions, 84%+ branches
- TypeScript strict mode compliance
- Zero linting errors

## [1.2.0](https://github.com/SylphxAI/pdf-reader-mcp/compare/v1.1.0...v1.2.0) (2025-10-31)

### Features

- **Content Ordering**: Preserve exact text and image order based on Y-coordinates
  - Content items within each page are now sorted by their vertical position
  - Enables AI to see content in the same order as it appears in the PDF
  - Text and images are interleaved based on document layout
  - Example: page 1 [text, image, text, image, image, text]
  - Uses PDF.js transform matrices to extract Y-coordinates
  - Automatically groups text items on the same line
  - Returns ordered content parts for optimal AI consumption

### Internal Changes

- New `extractPageContent()` function combines text and image extraction with positioning
- New `PageContentItem` interface tracks content type, position, and data
- Handler updated to generate content parts in document-reading order
- Improved error handling to return descriptive error messages as text content

### Code Quality

- All tests passing (91 tests)
- Coverage maintained at 97.76% statements, 90.95% branches
- TypeScript strict mode compliance
- Zero linting errors

## [1.1.0](https://github.com/SylphxAI/pdf-reader-mcp/compare/v1.0.0...v1.1.0) (2025-10-31)

### Features

- **Image Extraction**: Extract embedded images from PDF pages as base64-encoded data ([bd637f3](https://github.com/SylphxAI/pdf-reader-mcp/commit/bd637f3))
  - Support for RGB, RGBA, and Grayscale formats
  - Works with JPEG, PNG, and other embedded image types
  - Includes image metadata (width, height, format, page number)
  - Optional parameter `include_images` (default: false)
  - Uses PDF.js operator list API for reliable extraction

### Performance Improvements

- **Parallel Page Processing**: Process multiple pages concurrently for 5-10x speedup ([e5f85e1](https://github.com/SylphxAI/pdf-reader-mcp/commit/e5f85e1))
  - Refactored extractPageTexts to use Promise.all
  - 10-page PDF: ~5-8x faster
  - 50-page PDF: ~10x faster
  - Maintains error isolation per page

### Code Quality

- **Deep Architectural Refactoring**: Break down monolithic handler into focused modules ([1519fe0](https://github.com/SylphxAI/pdf-reader-mcp/commit/1519fe0))

  - handlers/readPdf.ts: 454 → 143 lines (-68% reduction)
  - NEW src/types/pdf.ts: Type definitions (44 lines)
  - NEW src/schemas/readPdf.ts: Zod schemas (61 lines)
  - NEW src/pdf/parser.ts: Page range parsing (124 lines)
  - NEW src/pdf/loader.ts: Document loading (74 lines)
  - NEW src/pdf/extractor.ts: Text & metadata extraction (96 lines → 224 lines with images)
  - Single Responsibility Principle applied throughout
  - Functional composition for better testability

- **Comprehensive Test Coverage**: 90 tests with 98.94% coverage ([85cf712](https://github.com/SylphxAI/pdf-reader-mcp/commit/85cf712))
  - NEW test/pdf/extractor.test.ts (22 tests)
  - NEW test/pdf/loader.test.ts (9 tests)
  - NEW test/pdf/parser.test.ts (26 tests)
  - Tests: 31 → 90 (+158% increase)
  - Coverage: 90.26% → 98.94% statements
  - Coverage: 78.64% → 93.33% branches

### Documentation

- Enhanced README with image extraction examples and usage guide
- Added dedicated Image Extraction section with format details
- Updated roadmap to reflect completed features
- Clarified image format support and considerations

## [1.0.0](https://github.com/SylphxAI/pdf-reader-mcp/compare/v0.3.24...v1.0.0) (2025-10-31)

### ⚠ BREAKING CHANGES

- **Package renamed from @sylphlab/pdf-reader-mcp to @sylphx/pdf-reader-mcp**
- Docker images renamed from sylphlab/pdf-reader-mcp to sylphx/pdf-reader-mcp

### Features

- Migrate from ESLint/Prettier to Biome for 50x faster linting ([bde79bf](https://github.com/SylphxAI/pdf-reader-mcp/commit/bde79bf))
- Add Docker and Smithery deployment support ([11dc08f](https://github.com/SylphxAI/pdf-reader-mcp/commit/11dc08f))

### Bug Fixes

- Fix Buffer to Uint8Array conversion for PDF.js v5.x compatibility ([1c7710d](https://github.com/SylphxAI/pdf-reader-mcp/commit/1c7710d))
- Fix schema validation with exclusiveMinimum for Mistral/Windsurf compatibility ([1c7710d](https://github.com/SylphxAI/pdf-reader-mcp/commit/1c7710d))
- Fix metadata extraction with robust .getAll() fallback ([1c7710d](https://github.com/SylphxAI/pdf-reader-mcp/commit/1c7710d))
- Fix nested test case that was not running ([2c8e1a5](https://github.com/SylphxAI/pdf-reader-mcp/commit/2c8e1a5))
- Update PdfSourceResult type for exactOptionalPropertyTypes compatibility ([4e0d81d](https://github.com/SylphxAI/pdf-reader-mcp/commit/4e0d81d))

### Improvements

- Upgrade all dependencies to latest versions ([dab3f13](https://github.com/SylphxAI/pdf-reader-mcp/commit/dab3f13))
  - @modelcontextprotocol/sdk: 1.8.0 → 1.20.2
  - pdfjs-dist: 5.1.91 → 5.4.296
  - All GitHub Actions updated to latest versions
- Rebrand from Sylphlab to Sylphx ([1b6e4d3](https://github.com/SylphxAI/pdf-reader-mcp/commit/1b6e4d3))
- Revise README for better clarity and modern structure ([b770b27](https://github.com/SylphxAI/pdf-reader-mcp/commit/b770b27))

### Migration Guide

To migrate from @sylphlab/pdf-reader-mcp to @sylphx/pdf-reader-mcp:

1. Uninstall old package:

   ```bash
   npm uninstall @sylphlab/pdf-reader-mcp
   ```

2. Install new package:

   ```bash
   npm install @sylphx/pdf-reader-mcp
   ```

3. Update your MCP configuration to use @sylphx/pdf-reader-mcp

4. If using Docker, update image name to sylphx/pdf-reader-mcp

All functionality remains the same. No code changes required.

### [0.3.24](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.23...v0.3.24) (2025-04-07)

### Bug Fixes

- enable rootDir and adjust include for correct build structure ([a9985a7](https://github.com/sylphlab/pdf-reader-mcp/commit/a9985a7eed16ed0a189dd1bda7a66feb13aee889))

### [0.3.23](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.22...v0.3.23) (2025-04-07)

### Bug Fixes

- correct executable paths due to missing rootDir ([ed5c150](https://github.com/sylphlab/pdf-reader-mcp/commit/ed5c15012b849211422fbb22fb15d8a2c9415b0b))

### [0.3.22](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.21...v0.3.22) (2025-04-07)

### [0.3.21](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.20...v0.3.21) (2025-04-07)

### [0.3.20](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.19...v0.3.20) (2025-04-07)

### [0.3.19](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.18...v0.3.19) (2025-04-07)

### [0.3.18](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.17...v0.3.18) (2025-04-07)

### Bug Fixes

- **publish:** remove dist from gitignore and fix clean script ([305e259](https://github.com/sylphlab/pdf-reader-mcp/commit/305e259d6492fbc1732607ee8f8344f6e07aa073))

### [0.3.17](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.16...v0.3.17) (2025-04-07)

### Bug Fixes

- **config:** align package.json paths with build output (dist/) ([ab1100d](https://github.com/sylphlab/pdf-reader-mcp/commit/ab1100d771e277705ef99cb745f89687c74a7e13))

### [0.3.16](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.15...v0.3.16) (2025-04-07)

### [0.3.15](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.14...v0.3.15) (2025-04-07)

### Bug Fixes

- Run lint-staged in pre-commit hook ([e96680c](https://github.com/sylphlab/pdf-reader-mcp/commit/e96680c771eb99ba303fdf7ad51da880261e11c1))

### [0.3.14](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.13...v0.3.14) (2025-04-07)

### [0.3.13](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.12...v0.3.13) (2025-04-07)

### Bug Fixes

- **docker:** Install pnpm globally in builder stage ([651d7ae](https://github.com/sylphlab/pdf-reader-mcp/commit/651d7ae06660b97af91c348bc8cc786613232c06))

### [0.3.11](https://github.com/sylphlab/pdf-reader-mcp/compare/v0.3.10...v0.3.11) (2025-04-07)

### [0.3.10](https://github.com/sylphlab/pdf-reader-mcp/compare/v1.0.0...v0.3.10) (2025-04-07)

### Bug Fixes

- address remaining eslint warnings ([a91d313](https://github.com/sylphlab/pdf-reader-mcp/commit/a91d313bec2b843724e62ea6a556d99d5389d6cc))
- resolve eslint errors in tests and scripts ([ffc1bdd](https://github.com/sylphlab/pdf-reader-mcp/commit/ffc1bdd18b972f58e90e12ed2394d2968c5639d9))

## [1.0.0] - 2025-04-07

### Added

- **Project Alignment:** Aligned project structure, configuration (TypeScript, ESLint, Prettier, Vitest), CI/CD (`.github/workflows/ci.yml`), Git Hooks (Husky, lint-staged, commitlint), and dependency management (Dependabot) with Sylph Lab Playbook guidelines.
- **Testing:** Achieved ~95% test coverage using Vitest.
- **Benchmarking:** Implemented initial performance benchmarks using Vitest `bench`.
- **Documentation:**
  - Set up documentation website using VitePress.
  - Created initial content for Guide, Design, Performance, Comparison sections.
  - Updated `README.md` to follow standard structure.
  - Added `CONTRIBUTING.md`.
  - Updated Performance page with initial benchmark results.
  - Added community links and call-to-action in VitePress config footer.
- **Package Manager:** Switched from npm to pnpm.

### Changed

- **Dependencies:** Updated various dependencies to align with guidelines and ensure compatibility.
- **Configuration:** Refined `tsconfig.json`, `eslint.config.js`, `vitest.config.ts`, `package.json` based on guidelines.
- **Project Identity:** Updated scope to `@sylphlab`.

### Fixed

- Resolved various configuration issues identified during guideline alignment.
- Corrected Markdown parsing errors in initial documentation.
- Addressed peer dependency warnings where possible.
- **Note:** TypeDoc API generation is currently blocked due to unresolved initialization errors with TypeDoc v0.28.1.

### Removed

- Sponsorship related files and badges (`.github/FUNDING.yml`).

## [0.3.9] - 2025-04-05

### Fixed

- Removed artifact download/extract steps from `publish-docker` job in workflow, as Docker build needs the full source context provided by checkout.

## [0.3.8] - 2025-04-05

### Fixed

- Removed duplicate `context: .` entry in `docker/build-push-action` step in `.github/workflows/publish.yml`.

## [0.3.7] - 2025-04-05

### Fixed

- Removed explicit `COPY tsconfig.json ./` from Dockerfile (rely on `COPY . .`).
- Explicitly set `context: .` in docker build-push action.

## [0.3.6] - 2025-04-05

### Fixed

- Explicitly added `COPY tsconfig.json ./` before `COPY . .` in Dockerfile to ensure it exists before build step.

## [0.3.5] - 2025-04-05

### Fixed

- Added `RUN ls -la` before build step in Dockerfile to debug `tsconfig.json` not found error.

## [0.3.4] - 2025-04-05

### Fixed

- Explicitly specify `tsconfig.json` path in Dockerfile build step (`RUN ./node_modules/.bin/tsc -p tsconfig.json`) to debug build failure.

## [0.3.3] - 2025-04-05

### Fixed

- Changed Dockerfile build step from `RUN npm run build` to `RUN ./node_modules/.bin/tsc` to debug build failure.

## [0.3.2] - 2025-04-05

### Fixed

- Simplified `build` script in `package.json` to only run `tsc` (removed `chmod`) to debug Docker build failure.

## [0.3.1] - 2025-04-05

### Fixed

- Attempted various fixes for GitHub Actions workflow artifact upload issue (`Error: Provided artifact name input during validation is empty`). Final attempt uses fixed artifact filename in upload/download steps.

## [0.3.0] - 2025-04-05

### Added

- `CHANGELOG.md` file based on Keep a Changelog format.
- `LICENSE` file (MIT License).
- Improved GitHub Actions workflow (`.github/workflows/publish.yml`):
  - Triggers on push to `main` branch and version tags (`v*.*.*`).
  - Conditionally archives build artifacts only on tag pushes.
  - Conditionally runs `publish-npm` and `publish-docker` jobs only on tag pushes.
  - Added `create-release` job to automatically create GitHub Releases from tags, using `CHANGELOG.md` for the body.
- Added version headers to Memory Bank files (`activeContext.md`, `progress.md`).

### Changed

- Bumped version from 0.2.2 to 0.3.0.

<!-- Note: Removed [0.4.0-dev] entry as changes are now part of 1.0.0 -->
