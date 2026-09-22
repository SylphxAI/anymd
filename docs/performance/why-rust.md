---
layout: doc
title: Why a native Rust engine
---

# Why a native Rust engine

> **Product status:** `@sylphx/citra` is the brand-sole **sole-Rust production** package for the five published native targets (thin Node launcher + one platform native binary).
> Users should not need to “opt in” to Rust. Missing natives fail closed.  
> Engineering residual catalogs below are **maintainer history**, not the product pitch.

## User-facing reason

PDF Reader MCP is a public MCP that agents call heavily. The right product is **one local engine** that returns evidence — not a migration story.

Native Rust is a **second-layer proof point**:

- Local-first execution
- Five supported platforms
- Clean install (empty production npm dependency graph)
- Fail-closed behavior when the platform binary is absent

Primary product promise remains:

> Give your AI agent eyes for PDFs.

See the [README](https://github.com/SylphxAI/citra#readme) and [installation guide](/guide/installation).

## Capability-first admission (ADR-0005)

Pure-Rust admission is **capability-first semantic compatibility**, not whole-product exact PDF.js output parity.

- Interface/schema/transport/error contracts remain exact.
- Semantic capability equivalence and calibrated agent-task quality are the release standards.
- Frozen TS v3.0.14 differentials remain valuable regression/coverage evidence.
- Exact residual families expand only for contract breaks, semantic regressions, or security/resource fail-closed gaps.

See `docs/adr/0005-capability-first-semantic-compatibility.md` and
`docs/specs/capability-first-admission-contract.md`.

## Engineering archive

The sections after this banner preserve historical residual notes and migration
context for maintainers. They are **not** acquisition copy. Live product install
truth is Citra 5.0.0 — see [migration notes](/migration).

---

## Historical product decision notes


`@sylphx/pdf-reader-mcp` is a **starred public MCP**. Agents call it thousands of
times. The right end state is not “half TypeScript, half Rust, please wait while
we migrate” — it is **one native engine** with the same public tools and the same
structured document result fields.

The withdrawn v3.1.x releases did **not** reach that end state. The intended
replacement, once it passes the executable parity and release gates, is:

- **npm**: `@sylphx/citra` → ships the native MCP server binary
- **Tools**: `read_pdf`, `search_pdf`, and every `pdf_evidence` success and failure path

## What must not regress

Updating the implementation language must **not** remove public capabilities.
The replacement must reproduce the Document Twin semantics, not merely return
fields with these names. Today many pure-Rust fields are heuristic or empty;
their current status is recorded in the
[capability matrix](../specs/pure-rust-capability-matrix.json).

| Capability group | Fields / operations |
| --- | --- |
| Core text | `full_text`, `markdown`, `html`, `chunks`, `elements`, `text_layer` |
| Structure | `tables`, `document_map`, `document_ast`, `layout_diagnostics` |
| Safety / trust | `safety_findings`, `trust_report`, `accessibility_report` |
| Document signals | `outline`, `annotations`, `form_fields`, `attachments`, `structure_trees`, `page_labels`, `page_geometry`, `permissions` |
| Provider opt-in | `ocr_text_layer` has a bounded command-provider fusion subset; without a provider the field is omitted with an explicit warning, matching TS failure semantics. `visual_enrichments` remains a placeholder. |
| Evidence | `pdf_evidence` `inspect` (routing); visual ops fail closed with guidance when no render/OCR backend is configured |

The following commands exercise only the currently claimed subset. They are
not a drop-in or release-admission proof:

```bash
bun run build:rust
bun test test/production/capabilityParity.contract.test.ts --timeout=600000
bun run check:production-contract
```

## Benchmark method

```bash
bun run build:rust
bun run benchmark:pure-rust -- --iterations 20 --warmup 3 \
  --output benchmark-artifacts/pdf_pure_rust_benchmark.json
```

Scenarios (fixed fixture `test/fixtures/sample.pdf`):

1. `metadata_page_count`
2. `full_text`
3. `agent_document_twin_balanced` (auto)
4. `agent_document_twin_full` (explicit include_*)
5. `search_literal`
6. `inspect`

Each scenario warms up, then records avg / min / max / p50 / p95 over N iterations
against the **production pure-Rust binary**.

Historical TypeScript timings are retained below only as provenance for an old
measurement. They were not collected in the same run, on a digest-bound
semantically equivalent candidate, and therefore cannot establish a speedup:

| Scenario | Historical TS avg |
| --- | --- |
| Metadata + page count | 1.1 ms |
| Full text | 16.1 ms |
| structured document result | 27.2 ms |

The pure-Rust harness currently writes a `comparison` block, but those ratios
are diagnostic historical comparisons and must not be used as release or
marketing claims.

## Historical diagnostic run (not an A/B benchmark)

Fixture: `test/fixtures/sample.pdf` · iterations=15 · warmup=3 · measuredAt=2026-07-17T23:15:41.531Z

| Scenario | Pure-Rust avg | Pure-Rust p50 | Pure-Rust p95 | Historical TS avg | Speedup |
| --- | ---: | ---: | ---: | ---: | ---: |
| Metadata + page count | 11.174 ms | 11.665 ms | 13.318 ms | 1.1 ms | 0.10× |
| Full text | 8.041 ms | 5.875 ms | 14.169 ms | 16.1 ms | 2.00× |
| structured document result (balanced) | 8.227 ms | 6.447 ms | 14.826 ms | 27.2 ms | 3.31× |
| structured document result (full includes) | 12.916 ms | 12.516 ms | 18.278 ms | — | — |
| search_pdf literal | 0.862 ms | 0.735 ms | 1.573 ms | — | — |
| pdf_evidence inspect | 9.325 ms | 9.158 ms | 13.206 ms | — | — |

No speedup claim is admissible from this table. A release benchmark must run
TS 3.0.14 and the exact Rust candidate on the same host and corpus, verify
semantically equivalent outputs before timing, randomize/interleave samples,
separate cold and warm paths, and retain raw samples plus source, fixture,
binary, toolchain, and environment digests.

Artifact: `benchmark-artifacts/pdf_pure_rust_benchmark.json`.


## Why performance work still matters

- **Agent loops are serial.** Verified latency reductions compound across tool calls.
- **Cold start matters for MCP.** It must be measured separately from warm calls.
- **Ops cost.** A native distribution can simplify deployment only after cross-platform packaging is proven.

## Capability honesty

Pure-Rust text extraction currently uses the selectable text layer. Geometry,
visual evidence, and full Document Twin parity remain open work. Empty
placeholder arrays prove only response shape; they do not prove capability.

Visual `pdf_evidence` operations now have bounded Hayro render/crop plus opt-in
command-provider OCR and region-analysis subsets. `read_pdf` can now select OCR
pages, invoke that bounded command provider, return the normalized parallel OCR
layer, link applied pages into `document_map`, emit OCR MCP text parts, and
project one bounded boxed-word table subset through tables, elements, chunks,
Markdown, HTML, AST, and map. The immutable detached TS 3.0.14 differential
covers these bounded fusion cases. It remains partial: mixed selectable/OCR
table continuation, full AST hierarchy, OCR TSV breadth beyond admitted subsets,
broader renderer fixtures, and full Document Twin fusion are open. Unavailable
paths fail closed; this is not TS 3.0.14 parity.

## Install

Production: pin `@sylphx/citra` to a released version (sole-Rust production; no TypeScript production runtime).
See [installation guide](../guide/installation.md). The parity residuals below are
historical engineering evidence, not an alternate production runtime.

### Configured-command visual enrichment fusion (bounded)

A frozen five-case configured-command `read_pdf` visual-enrichment payload and Document Map fusion subset is admitted through the exact-SHA differential harness. It covers direct table fusion, page sort/dedup/invalid + max-one admission, ordered two-caption analyses, fail-closed second-provider failure, and ready/no-candidate zero invocation. Pure-Rust also implements analyze_regions HTTP URL and ollama/openai-compatible/lmstudio/llamacpp presets with local mock proof. `include_visual_enrichments` remains `PARTIAL` while public remote-provider fidelity and whole-product parity stay unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### Document AST visual enrichment fusion (bounded)

A frozen five-case configured-command `read_pdf` visual-enrichment Document AST fusion subset is admitted through the exact-SHA differential harness. Direct table attachment, caption-derived orphan figure/chart nodes, fail-closed provider failure, and ready/no-candidate zero invocation are proven. `include_document_ast` and `include_visual_enrichments` remain `PARTIAL`; Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_ocr_text_layer public-stdio fusion (bounded)

A frozen six-case public-stdio `read_pdf` `include_ocr_text_layer` subset is admitted through the exact-SHA differential harness. It covers opt-out, page-1 success with Document Map OCR fields, page-3 multiword boxes, invalid page-selection warning with page-1 OCR retained, provider-failure soft warning without layer, and not-configured soft warning without layer. Leaf-mutation count is frozen at 143 with relocated fixture-root replay. `include_ocr_text_layer` remains `PARTIAL`; tesseract-tsv, text-only fallback, selectable/OCR table continuation, URL single-fetch, first-five-of-six page boundary, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf OCR residual public-stdio subset (bounded)

A frozen three-case public-stdio residual for `read_pdf` `include_ocr_text_layer` is admitted: plain-text provider stdout without words, JSON text-only without words, and default `max_pages` first-five-of-six truncation with exact warning and five applied OCR pages. Leaf-mutation count is frozen at 154 with relocated fixture-root replay. Document Map `needs_ocr_pages` remains layout-derived and is not overwritten by OCR candidates. `include_ocr_text_layer` remains `PARTIAL`; tesseract-tsv, selectable/OCR table continuation, URL single-fetch, mixed interleaving, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf tesseract-tsv public-stdio subset (bounded)

A frozen two-case public-stdio `MCP_PDF_OCR_PRESET=tesseract-tsv` subset is admitted: valid TSV level-5 words with image-to-PDF box conversion, and malformed TSV soft raw fallback with exact warning. Leaf-mutation count is frozen at 66. Real tesseract binary health checks remain unclaimed. `include_ocr_text_layer` remains `PARTIAL`; Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### mixed selectable/OCR table merge public-stdio subset (bounded)

A frozen three-case public-stdio mixed selectable/OCR table merge subset is admitted: distinct non-overlapping OCR tables are reindexed after selectable tables, overlapping OCR tables are suppressed at the 0.6 IoU duplicate threshold, and OCR-only pages without selectable tables still project table_info. Leaf-mutation count is frozen at 76. `include_tables` and `include_ocr_text_layer` remain `PARTIAL`; Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### OCR-search residual public-stdio subset (bounded)

A frozen four-case public-stdio residual for `search_pdf` `include_ocr_text_layer` is admitted: plain-text OCR fallback without match geometry, JSON text-only without words/geometry, words control with `ocr_word` geometry, and `max_pages` first-five-of-six truncation with exact warning. Leaf-mutation count is frozen at 133. `include_ocr_text_layer` remains `PARTIAL`; selectable/OCR interleaving and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### selectable/OCR search interleaving public-stdio subset (bounded)

A frozen four-case public-stdio selectable/OCR search interleaving subset is admitted: selectable matches append before OCR matches, exact `max_matches` admits one OCR match, a full selectable cap skips OCR without a truncation flag, and unique OCR-only tokens append as sole matches. Leaf-mutation count is frozen at 149. `include_ocr_text_layer` remains `PARTIAL`; Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### URL source single-fetch public-stdio subset (bounded)

A frozen two-case public-stdio URL source single-fetch subset is admitted for `read_pdf` over a local fixture HTTP server with `MCP_PDF_ALLOW_PRIVATE_IPS=true`: no-OCR single fetch and OCR-with-document-map single fetch that reuses materialized bytes. Leaf-mutation count is frozen at 16 with relocated fixture-root replay. `url_ssrf` remains `PARTIAL`; `search_pdf` URL+OCR single-fetch (TS double-fetch residual), resolver timeout, TLS-SNI fixtures, public-internet fetch, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### search_pdf prefer_speed route (post-3.0.14 surface)

Pure-Rust `search_pdf` now accepts `prefer_speed` and, when OCR is off, matches the current TypeScript speed route: match geometry is omitted, provenance uses `rust-text-index`/`text-content`, and the speed-route warning is emitted. This is a current-surface parity fix, not a frozen detached TS v3.0.14 differential claim. `prefer_speed` remains `PARTIAL` in the capability matrix; Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### search_pdf multiword selectable geometry residual (bounded)

A frozen three-case public-stdio `search_pdf` multiword selectable-text geometry residual is admitted over `v3014-behavior-v1.pdf`: start-item multiword box union, mid-line multiword union, and case-insensitive multiword, all at `char_estimated` level with float-normalized boxes. Leaf-mutation count is frozen at 48 with relocated fixture-root replay. `bounding_box` remains `PARTIAL`; glyph-perfect boxes, RTL/vertical writing, OCR fusion, prefer_speed, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf form residual public-stdio subset (bounded)

A frozen two-case public-stdio `read_pdf` `include_form_fields` residual is admitted: radiobutton/pushbutton/signature kinds (signature omits box and editable), and pdf.js-compatible value coercion for numeric text, missing text defaults, bool button checkbox Off, and choice array first-string selection. Leaf-mutation count is frozen at 90 with relocated fixture-root replay. `include_form_fields` remains `PARTIAL`; radio group kids, broader button-array coercion, malformed-field breadth, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf form radio-group residual (bounded)

A frozen two-case public-stdio `read_pdf` `include_form_fields` radio-group residual is admitted: parent stub plus radiobutton kids inheriting `V` (with sibling checkbox control), and a three-option radio group inheriting value `Silver` and default `Bronze`. Leaf-mutation count is frozen at 78 with relocated fixture-root replay. `include_form_fields` remains `PARTIAL`; broader button-array coercion, malformed-field breadth, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf attachment residual public-stdio subset (bounded)

A frozen two-case public-stdio `read_pdf` `include_attachments` residual is admitted: EmbeddedFiles name-tree `Kids` win over sibling `Names`, trailing-slash `UF` becomes `unnamed`, and Windows-path `UF` basenames. Leaf-mutation count is frozen at 17 with relocated fixture-root replay. `include_attachments` remains `PARTIAL`; malformed name-tree breadth, duplicate-kids fail-closed, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf mark_info residual public-stdio subset (bounded)

A frozen three-case public-stdio catalog `mark_info` residual is admitted: non-boolean `Suspects` defaults to false with missing `UserProperties` false, all-true MarkInfo, and empty MarkInfo all-false. Leaf-mutation count is frozen at 30 with relocated fixture-root replay. Catalog surfaces remain `PARTIAL`; permissions encryption, outline beyond behavior, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf form parent/child residual (bounded)

A frozen two-case public-stdio `include_form_fields` parent/child residual is admitted: skip top-level and Kids direct dicts with parent stub plus dotted spaced child name and DV-as-value, and readonly parent `Ff` inheritance making child non-editable. Leaf-mutation count is frozen at 34 with relocated fixture-root replay. `include_form_fields` remains `PARTIAL`; malformed-field breadth, broader button-array coercion, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf annotation residual public-stdio subset (bounded)

A frozen two-case public-stdio `read_pdf` `include_annotations` residual is admitted: FreeText and Link URI keep full `/Rect` boxes including multi-page FreeText, while Text sticky notes claim contents/title only without bounding boxes (pdf.js icon-box geometry remains unclaimed). Leaf-mutation count is frozen at 44 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; dest-only Link page-ref shape, glyph-perfect boxes, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf annotation dest residual public-stdio subset (bounded)

A frozen two-case public-stdio `read_pdf` `include_annotations` dest residual is admitted: dest-only Link keeps pdf.js page-ref `{num,gen}` plus name token `{name}` for `/Fit` and `/XYZ` coordinate destinations with full `/Rect` boxes. Leaf-mutation count is frozen at 33 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; named dest string lookup, GoTo action dest, Text icon/box geometry, glyph-perfect boxes, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf annotation action/named dest residual public-stdio subset (bounded)

A frozen two-case public-stdio `read_pdf` `include_annotations` action/named dest residual is admitted: named `/Dest` string is preserved, and GoTo action `/D` is exposed as pdf.js `dest` with page-ref `{num,gen}`, name token `FitH`, and coordinate, with full `/Rect` boxes. Leaf-mutation count is frozen at 29 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; named dest resolution to array, remote GoToR, URI action beyond url, Text icon/box geometry, glyph-perfect boxes, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf annotation action-precedence residual public-stdio subset (bounded)

A frozen three-case public-stdio `read_pdf` `include_annotations` action-precedence residual is admitted: GoTo action dest wins over explicit `/Dest` (`FitH` + coordinate), URI action exposes `url` and suppresses `/Dest`, and Launch file maps to `url` with no dest, all with full `/Rect` boxes. Leaf-mutation count is frozen at 42 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; named dest resolution to array, remote GoToR, Launch file-dict variants, Text icon/box geometry, glyph-perfect boxes, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf info flags residual public-stdio subset (bounded)

A frozen two-case public-stdio `read_pdf` `include_metadata` info residual is admitted: pdf.js info flags (`Language`, `EncryptFilterName`, `IsLinearized`, `IsAcroFormPresent`, `IsXFAPresent`, `IsCollectionPresent`, `IsSignaturesPresent`) plus document info fields for AcroForm+Lang and plain catalogs. Leaf-mutation count is frozen at 33 with relocated fixture-root replay. `include_metadata` remains `PARTIAL`; XMP metadata payload, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf page_geometry residual public-stdio subset (bounded)

A frozen three-case public-stdio `read_pdf` `include_page_geometry` residual is admitted: Rotate+UserUnit+CropBox dimensions, default MediaBox identity geometry, and inverted MediaBox normalization with `view_box`. Leaf-mutation count is frozen at 39 with relocated fixture-root replay. `include_page_geometry` remains `PARTIAL`; Bleed/Trim/Art boxes, non-right-angle rotation, inherited MediaBox breadth, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.



### read_pdf include_page_geometry inheritance residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_page_geometry` inheritance residual is admitted: pdf.js inherits `MediaBox`/`Rotate` from `Pages`, intersects page `CropBox` with inherited `MediaBox` for `view_box`, and normalizes negative right-angle `Rotate` (`-90` → `270`) with width/height swap. Leaf-mutation count is frozen at 39 with relocated fixture-root replay. Bleed/Trim/Art boxes, non-right-angle rotation beyond clamp, deeper Pages inheritance breadth, and whole-product parity remain unclaimed; `include_page_geometry` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_page_geometry depth-clamp residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_page_geometry` depth-clamp residual is admitted: pdf.js deeper Pages `MediaBox`/`CropBox` inheritance across an intermediate Pages node, non-right-angle `Rotate` `45` clamps to `0`, and partial `MediaBox`/`CropBox` intersection projects the clipped `view_box`. Leaf-mutation count is frozen at 39 with relocated fixture-root replay. Bleed/Trim/Art boxes, UserUnit Pages inheritance, multi-page geometry breadth, and whole-product parity remain unclaimed; `include_page_geometry` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_page_geometry userunit-multipage residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_page_geometry` userunit-multipage residual is admitted: pdf.js does not inherit Pages `UserUnit` onto page geometry, multi-page selected geometry projects per-page `MediaBox`/`Rotate` (including 90-degree width/height swap), and Bleed/Trim/Art boxes do not replace `MediaBox` `view_box`. Leaf-mutation count is frozen at 48 with relocated fixture-root replay. Bleed/Trim/Art as alternate view source, page-level UserUnit override breadth beyond the frozen fixtures, large multi-page breadth, and whole-product parity remain unclaimed; `include_page_geometry` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf page_labels residual public-stdio subset (bounded)

A frozen three-case public-stdio `read_pdf` `include_page_labels` residual is admitted: multi-style PageLabels (prefix roman, decimal start, alpha), prefix+start decimal sequence, and absent labels omitted. Leaf-mutation count is frozen at 17 with relocated fixture-root replay. `include_page_labels` remains `PARTIAL`; number-tree Kids breadth, hostile label overflow, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf outline residual public-stdio subset (bounded)

A frozen three-case public-stdio `read_pdf` `include_outline` residual is admitted: URI outline with bold/italic/color and child Fit dest (`dest` null on URI parent), FitH dest with coordinate, and absent outline omitted. Leaf-mutation count is frozen at 39 with relocated fixture-root replay. `include_outline` remains `PARTIAL`; outline cycle suppression beyond behavior, named dest string lookup, remote GoToR, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf permissions residual public-stdio subset (bounded)

A frozen four-case public-stdio `read_pdf` `include_permissions` residual is admitted: empty-user-password encrypted PDFs expose pdf.js permission labels for print/copy/fill/a11y, modify/annotate/assemble, and print+print_high_quality; unencrypted PDFs omit permissions. Leaf-mutation count is frozen at 25 with relocated fixture-root replay. `include_permissions` remains `PARTIAL`; non-empty user password, owner-only unlock, unknown permission bits, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf metadata presence residual public-stdio subset (bounded)

A frozen two-case public-stdio `read_pdf` `include_metadata` presence residual is admitted: catalog without `/Metadata` omits `data.metadata`; catalog with `/Metadata` stream emits an empty metadata object matching pdfjs-dist Node presence (no synthetic info wrapper). Leaf-mutation count is frozen at 12 with relocated fixture-root replay. `include_metadata` remains `PARTIAL`; XMP key/value parsing, rich getAll payloads, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_metadata info-extras residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_metadata` info-extras residual is admitted: `data.info` key sets match pdf.js `getMetadata().info` with no rust-only extras (`text_chars`, nested `route`, nested `num_pages`) for AcroForm+Lang and plain catalogs, while top-level `num_pages` remains separate. Leaf-mutation count is frozen at 60 with relocated fixture-root replay. `include_metadata` remains `PARTIAL`; XMP key/value parsing, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_metadata info trapped-custom residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_metadata` info trapped-custom residual is admitted: pdf.js `getMetadata().info` projects `Trapped` Name as `{name}` and non-standard Info keys under `Custom` (string/number/boolean/Name). Leaf-mutation count is frozen at 48 with relocated fixture-root replay. XMP key/value parsing, keywords array coercion, info beyond Trapped/Custom breadth, and whole-product parity remain unclaimed; `include_metadata` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_metadata encrypt-filter residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_metadata` encrypt-filter residual is admitted: empty-user-password Standard-encrypted PDFs expose `EncryptFilterName=Standard`, and unencrypted PDFs keep `EncryptFilterName` null, matching pdf.js `getMetadata().info`. Leaf-mutation count is frozen at 18 with relocated fixture-root replay. `include_metadata` remains `PARTIAL`; non-Standard security handlers, non-empty user password, linearized-true, signatures-true, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_metadata linearized residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_metadata` linearized residual is admitted: pdf.js `IsLinearized` matches `Linearization.create` over exact source bytes — valid first-object linearization dictionaries are true; spurious `Linearized` dictionaries (bad `L`/`H`) and absent linearization are false. Leaf-mutation count is frozen at 27 with relocated fixture-root replay. `include_metadata` remains `PARTIAL`; four-entry hint arrays, pageFirst optional variants, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_metadata form-flags residual (frozen TS v3.0.14)

A frozen four-case public-stdio `read_pdf` `include_metadata` form-flags residual is admitted matching pdf.js `formInfo`: XFA-only AcroForm yields `IsAcroFormPresent=false` + `IsXFAPresent=true`; Collection catalogs set `IsCollectionPresent=true`; visible Sig fields set `IsSignaturesPresent=true` + `IsAcroFormPresent=true`; invisible-only document signatures set `IsSignaturesPresent=true` + `IsAcroFormPresent=false`. Leaf-mutation count is frozen at 36 with relocated fixture-root replay. `include_metadata` remains `PARTIAL`; nested Kids signature-only breadth, empty XFA stream edge cases, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_annotations text-annotation residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_annotations` text-annotation residual is admitted: Text annotations without appearance use pdf.js 22×22 icon boxes (`bottom = top - 22`, `right = left + 22`); FreeText keeps raw `/Rect` geometry. Leaf-mutation count is frozen at 27 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; Text with appearance stream, Name field, glyph-perfect boxes, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_annotations remote-action residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` remote-action residual is admitted: Launch string files map to `url`; Launch file dictionaries prefer `UF` over `F`; GoToR builds remote `url` as `file#` + JSON explicit dest and suppresses `dest`. Leaf-mutation count is frozen at 42 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; GoToE attachments, `newWindow`, named remote dest string variants, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_annotations popup-annotation residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_annotations` popup residual is admitted: Popup inherits parent Text `title`/`contents` (pdf.js PopupAnnotation parent projection); parent Text keeps no-appearance 22×22 icon box; FreeText control keeps raw `/Rect` without parent inheritance. Leaf-mutation count is frozen at 36 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; Popup Group/IRT chain, zero-size rect nulling, richText RC, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_annotations popup-zero-size residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_annotations` popup zero-size residual is admitted: Popup with zero width or height nulls `rect` (omits `bounding_box`) like pdf.js `PopupAnnotation`; nonzero Popup keeps its `bounding_box`; zero-size Popup still inherits parent Text `title`/`contents`. Leaf-mutation count is frozen at 42 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; Popup Group/IRT chain, richText RC, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_annotations popup-group-irt residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_annotations` popup group/IRT residual is admitted: Markup Text with `RT=Group` inherits `title`/`contents` from the `IRT` root (pdf.js MarkupAnnotation overwrite); Popup whose Parent has `RT=Group` also projects the IRT root `title`/`contents`; non-group Popup control keeps parent Text inheritance. Leaf-mutation count is frozen at 64 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; richText RC, group color/flags, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_annotations text-appearance residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_annotations` text-appearance residual is admitted: Text annotations with `AP/N` stream keep raw `/Rect` geometry (pdf.js `hasAppearance`); empty appearance streams still count as appearance and do not force the 22×22 icon box. Leaf-mutation count is frozen at 28 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; Name field, glyph-perfect boxes, named appearance-state selection, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_annotations text-named-appearance residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_annotations` text named-appearance residual is admitted: Text `AP/N` named-state dictionaries require `AS` selection for `hasAppearance` (pdf.js `setAppearance`); `AS`-selected streams keep raw `/Rect`, while missing `AS` falls back to the 22×22 icon box. Leaf-mutation count is frozen at 28 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; Name field, glyph-perfect boxes, invalid-AS fallback variants, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_annotations text-inverted-rect residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_annotations` text inverted-rect residual is admitted: Text without appearance normalizes inverted `/Rect` first (pdf.js `lookupNormalRect`) then applies the 22×22 icon box from the normalized top-left; ordinary no-appearance Text keeps the same icon-box rule. Leaf-mutation count is frozen at 28 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; Name field, glyph-perfect boxes, appearance-stream geometry, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_annotations remote-named-dest residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` remote named-dest residual is admitted: GoToR with named dest string and PDF name-object `D` append as `file#name` (pdf.js `fetchRemoteDest`) and suppress `dest`; explicit-array GoToR control keeps `file#` + JSON dest. Leaf-mutation count is frozen at 42 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; GoToE attachments, `newWindow`, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_page_labels kids residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_page_labels` residual is admitted: PageLabels number-tree `Kids` nodes expand like pdf.js (internal Kids ignore sibling Nums), multi-style control remains stable, and absent labels omit `page_labels`. Leaf-mutation count is frozen at 18 with relocated fixture-root replay. `include_page_labels` remains `PARTIAL`; hostile label overflow and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_form_fields button-array residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_form_fields` residual is admitted: checkbox/radiobutton/pushbutton non-empty `/V` name arrays preserve pdf.js string arrays, plain string button values remain strings, empty button arrays collapse to `Off`, and absent `/V` falls back to a non-empty `/DV` array for both `value` and `default_value`. Leaf-mutation count is frozen at 68 with relocated fixture-root replay. `include_form_fields` remains `PARTIAL`; malformed-field breadth, nested Kids beyond the parent/child residual, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_attachments attachment odd-names residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_attachments` attachment odd-names residual is admitted: odd-length EmbeddedFiles `Names` arrays keep complete key/value pairs and materialize a trailing unpaired key as `filename=unnamed` without `size_bytes` (pdf.js NameTree), covering orphan-only and pair-plus-orphan fixtures. Leaf-mutation count is frozen at 15 with relocated fixture-root replay. `include_attachments` remains `PARTIAL`; broader malformed name-tree breadth, duplicate-kids fail-closed, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_attachments attachment dupkids residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_attachments` attachment dupkids residual is admitted: a valid EmbeddedFiles name-tree Kids leaf materializes attachments, while duplicate Kids refs and self-cycle Kids fail closed (omit `attachments`) like pdf.js NameTree `FormatError`. Leaf-mutation count is frozen at 15 with relocated fixture-root replay. Broader malformed name-tree breadth, invalid UF fallback breadth, and whole-product parity remain unclaimed; `include_attachments` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_form_fields form utf16-text residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_form_fields` form utf16-text residual is admitted: pdf.js `stringToPDFString` decoding for form `V`/`DV` — valid UTF-16BE BOM, odd-length UTF-16BE drops the trailing unpaired byte, UTF-8 BOM, and PDFDocEncoding plain control. Leaf-mutation count is frozen at 52 with relocated fixture-root replay. `include_form_fields` remains `PARTIAL`; split-surrogate wire parity, metadata/annotation UTF-16 breadth, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_form_fields form text-multiline residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form text-multiline residual is admitted: pdf.js `stringToPDFString` preserves multiline Tx field values for escaped LF, escaped CRLF, and raw LF inside PDF literal strings. Leaf-mutation count is frozen at 45 with relocated fixture-root replay. Password/fileSelect flags, richText/DA, annotation/metadata multiline breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_annotations contents-multiline residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` contents-multiline residual is admitted: pdf.js `stringToPDFString` preserves multiline FreeText and Text annotation `Contents` for escaped LF and escaped CRLF. FreeText keeps raw Rect geometry; Text without appearance keeps the 22×22 icon box. Leaf-mutation count is frozen at 42 with relocated fixture-root replay. RichText RC fallback, default appearance DA, appearance-stream content bbox, form-field multiline breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_outline cycle residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_outline` cycle residual is admitted: pdf.js global processed-set outline cycle suppression for `Next` self-cycle (single item), sibling `Next` cycle (both siblings once), and self `First` child cycle (no nested items). Leaf-mutation count is frozen at 49 with relocated fixture-root replay. Named dest string lookup, remote GoToR, outline beyond cycle breadth, and whole-product parity remain unclaimed; `include_outline` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_outline named-dest residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_outline` named-dest residual is admitted: pdf.js keeps named outline `Dest`/action `D` as destination strings for literal string, name token, and GoTo `D` string without resolving `Names/Dests` to arrays. Leaf-mutation count is frozen at 33 with relocated fixture-root replay. Named dest resolution to array, remote GoToR, outline beyond named-dest breadth, and whole-product parity remain unclaimed; `include_outline` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_outline GoToR residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_outline` GoToR residual is admitted: pdf.js outline `GoToR` projects absolute-safe `url` from `F` + `#` + remote `D` for string, name token, and explicit-array destinations while forcing `dest` null. Leaf-mutation count is frozen at 36 with relocated fixture-root replay. Relative GoToR file urls, named dest array resolution, outline beyond GoToR breadth, and whole-product parity remain unclaimed; `include_outline` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_outline Launch residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_outline` Launch residual is admitted: pdf.js outline `Launch` projects absolute-safe `url` from `F` + `#` + remote `D` for string, filespec `UF` preference, and explicit-array destinations while forcing `dest` null (same `parseDestDictionary` path as `GoToR`). Leaf-mutation count is frozen at 36 with relocated fixture-root replay. Relative Launch file urls, `newWindow`, outline beyond Launch breadth, and whole-product parity remain unclaimed; `include_outline` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_outline relative-remote residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_outline` relative-remote residual is admitted: pdf.js outline `Launch`/`GoToR` drop relative `F` file specs (no absolute-safe `url`) and force `dest` null even when outline `Dest` is present. Leaf-mutation count is frozen at 33 with relocated fixture-root replay. Absolute remote breadth, `newWindow`, named dest array resolution, outline beyond relative-remote breadth, and whole-product parity remain unclaimed; `include_outline` remains `PARTIAL`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### public-stdio utf16-text residual (frozen TS v3.0.14)

A frozen three-case public-stdio utf16-text residual is admitted: pdf.js `stringToPDFString` odd-length UTF-16BE trailing-byte drop for Text annotation title/contents, FreeText contents, outline titles, and metadata `Title`/`Author`. Leaf-mutation count is frozen at 41 with relocated fixture-root replay. Form-field UTF-16 decoding remains covered by the form utf16-text residual. `include_annotations`/`include_outline`/`include_metadata` remain `PARTIAL`; split-surrogate wire parity, XMP key/value, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_annotations text invalid-as residual (frozen TS v3.0.14)

A frozen two-case public-stdio `read_pdf` `include_annotations` text invalid-as residual is admitted: Text `AP/N` named-state dictionaries with invalid `AS` selection fall back to the 22×22 icon box (pdf.js `setAppearance`) for missing `AS` name and `AS` pointing at a non-stream. Leaf-mutation count is frozen at 28 with relocated fixture-root replay. Valid `AS` raw-rect selection remains covered by the named-appearance residual. `include_annotations` remains `PARTIAL`; Name field, glyph-perfect boxes, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### read_pdf include_annotations line annotation residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` line annotation residual is admitted: Line annotations without appearance expand the public bounding box like pdf.js `LineAnnotation` — default border width 1 expansion, non-intersecting `Rect` replaced from `/L` with `2*width` then public expand by width, and `BS/W=2` expansion. Leaf-mutation count is frozen at 39 with relocated fixture-root replay. `include_annotations` remains `PARTIAL`; line endings `LE`, appearance-stream geometry, PolyLine/Polygon, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### read_pdf include_annotations polyline/polygon residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` polyline/polygon residual is admitted: PolyLine/Polygon annotations without appearance replace a non-intersecting public bounding box from the vertices bbox expanded by `2*borderWidth` like pdf.js `PolylineAnnotation`/`PolygonAnnotation` — default border width 1, Polygon subtype, and `BS/W=2` with a non-clamped Rect. Leaf-mutation count is frozen at 39. Line endings `LE`, appearance-stream geometry, Ink, tiny-Rect border-width clamp, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.



### read_pdf include_annotations ink annotation residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` ink annotation residual is admitted: Ink annotations without appearance replace a non-intersecting public bounding box from the InkList points bbox expanded by `2*borderWidth` like pdf.js `InkAnnotation` — default border width 1, multi-stroke union, and `BS/W=2` with a non-clamped Rect. Leaf-mutation count is frozen at 39. Appearance-stream geometry, tiny-Rect border-width clamp, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.



### read_pdf include_annotations border-width clamp residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` border-width clamp residual is admitted: when `BS/W` exceeds half of either `Rect` dimension, pdf.js `AnnotationBorderStyle.setWidth` clamps width to 1 before Line/PolyLine/Ink public bounding-box expansion. Tiny non-intersecting Rect fixtures prove the clamp for PolyLine, Line, and Ink. Leaf-mutation count is frozen at 39. Border array form, zero-size Rect clamp-bypass breadth, appearance-stream geometry, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.



### read_pdf include_annotations border-array width residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` border-array width residual is admitted: when `BS` is absent, pdf.js `Annotation.setBorderStyle` takes width from `Border` array index 2 and uses it for Line/PolyLine/Ink public bounding-box expansion. Large non-intersecting Rect fixtures prove PolyLine `Border [0 0 2]`, Line `Border [0 0 2]`, and Ink `Border [0 0 3]`. Leaf-mutation count is frozen at 39. Border array length < 3, BS preference over Border, zero-size Rect clamp-bypass breadth, appearance-stream geometry, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.



### read_pdf include_annotations border BS preference residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` border BS preference residual is admitted: when both `BS` and `Border` are present, pdf.js `Annotation.setBorderStyle` prefers the `BS` dictionary width `W` over `Border[2]` before Line/PolyLine/Ink public bounding-box expansion. Large non-intersecting Rect fixtures prove PolyLine/Line `BS/W=2` over `Border [0 0 9]` and Ink `BS/W=3` over `Border [0 0 9]`. Leaf-mutation count is frozen at 39. Border array length < 3, zero-size Rect clamp-bypass breadth, appearance-stream geometry, non-dict BS fallthrough breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations border BS nondict residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` border BS nondict residual is admitted: when the `BS` key is present but is not a usable Border-style dictionary (here `null`), pdf.js `Annotation.setBorderStyle` does **not** fall through to `Border[2]` and keeps the default width 1 before Line/PolyLine/Ink public bounding-box expansion. Large non-intersecting Rect fixtures prove PolyLine/Line/Ink with `BS null` over `Border [0 0 9]`. Leaf-mutation count is frozen at 39. Wrong-Type BS breadth, Border array length < 3, zero-size Rect clamp-bypass breadth, appearance-stream geometry, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations border array short residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` border array short residual is admitted: when `BS` is absent and `Border` has length &lt; 3, pdf.js `Annotation.setBorderStyle` does not read a missing width index — empty arrays call `setWidth(0)` and short non-empty arrays leave the default width — and drawing uses `width || 1` before Line/PolyLine/Ink public bounding-box expansion. Large non-intersecting Rect fixtures prove PolyLine `Border [0 0]`, Line `Border []`, and Ink `Border [0]`. Leaf-mutation count is frozen at 39. BS-present short Border combinations, zero-size Rect clamp-bypass breadth, appearance-stream geometry, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations border BS wrong-type residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` border BS wrong-type residual is admitted: when `BS` is a dictionary whose `Type` is present and not `Border`, pdf.js `Annotation.setBorderStyle` ignores `BS/W` and does not fall through to `Border[2]`, keeping default width 1 before Line/PolyLine/Ink public bounding-box expansion. Large non-intersecting Rect fixtures prove PolyLine/Line/Ink with `BS Type /NotBorder W 9` over `Border [0 0 5]`. Leaf-mutation count is frozen at 39. Missing-Type BS (uses W), zero-size Rect clamp-bypass breadth, appearance-stream geometry, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations border zero-size clamp-bypass residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` border zero-size clamp-bypass residual is admitted: when `Rect` has a zero dimension, pdf.js `AnnotationBorderStyle.setWidth` does not clamp `BS/W` because the half-dimension gate requires both maxWidth and maxHeight &gt; 0. Line/PolyLine/Ink public bounding-box expansion therefore uses the unclamped width. Fixtures prove PolyLine/Line zero-height `Rect` + `BS/W=2` and Ink zero-width `Rect` + `BS/W=2` on non-intersecting geometry. Leaf-mutation count is frozen at 39. Positive-dimension clamp cases, appearance-stream geometry, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations appearance bbox residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` appearance bbox residual is admitted: when `AP/N` is present, Line/PolyLine/Ink keep the raw `Rect` public bounding box and skip the no-appearance L/vertices/InkList expansion path used by pdf.js when appearance is absent. Large non-intersecting Rect fixtures with empty `AP/N` streams and `BS/W=2` prove Line/PolyLine/Ink keep `{200,200,300,300}` instead of expanded geometry boxes. Leaf-mutation count is frozen at 39. Appearance-stream content bbox derivation, line endings `LE`, named appearance states, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations AP non-stream residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` AP non-stream residual is admitted: when `AP/N` is present but is not a usable appearance stream (`null` or name), pdf.js `Annotation.setAppearance` leaves appearance unset, so Line/PolyLine/Ink still apply no-appearance L/vertices/InkList bounding-box expansion with `BS/W`. Fixtures prove Line/Ink `AP/N null` and PolyLine `AP/N /NotAStream` with non-intersecting large Rect and `BS/W=2` expand geometry instead of keeping raw Rect. Leaf-mutation count is frozen at 39. Named appearance-state selection, appearance-stream content bbox derivation, line endings `LE`, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations AP named-state residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` AP named-state residual is admitted: when `AP/N` is a named-state dictionary, pdf.js `Annotation.setAppearance` selects the `AS` stream (appearance present → Line keeps raw Rect) and leaves appearance unset when `AS` is missing or does not select a stream (Line still expands L geometry with `BS/W`). Fixtures prove Line `AS /On` keeps `{200,200,300,300}` while missing/`Off` AS expand to `{4,4,106,86}` with `BS/W=2` on non-intersecting large Rect. Leaf-mutation count is frozen at 39. Appearance-stream content bbox derivation, line endings `LE`, PolyLine/Ink named-state breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations AP named-state polyline/ink residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` AP named-state polyline/ink residual is admitted: PolyLine/Ink `AP/N` named-state dictionaries follow pdf.js `Annotation.setAppearance` AS selection — `AS` stream keeps raw Rect; missing/invalid AS leaves appearance unset so PolyLine/Ink expand vertices/InkList with `BS/W`. Fixtures prove PolyLine `AS /On` keeps `{200,200,300,300}`, Ink missing AS expands to `{26,26,104,94}`, and PolyLine invalid AS expands to `{6,6,104,84}` with `BS/W=2` on non-intersecting large Rect. Leaf-mutation count is frozen at 39. Appearance-stream content bbox derivation, line endings `LE`, Square/Circle named-state breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations AP named-state square/circle residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` AP named-state square/circle residual is admitted: Square/Circle `AP/N` named-state dictionaries follow pdf.js `Annotation.setAppearance` AS selection — `AS` stream keeps raw Rect; missing/invalid AS leaves appearance unset but Square/Circle still keep raw Rect (no Line/PolyLine/Ink-style geometry expansion) with `BS/W` present. Fixtures prove Square `AS /On` keeps `{200,200,300,300}`, Circle missing AS keeps `{50,60,150,160}`, and Square invalid AS keeps `{10,20,110,120}` with `BS/W=2`. Leaf-mutation count is frozen at 39. Appearance-stream content bbox derivation, line endings `LE`, Widget named-state breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations highlight quadpoints residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` highlight quadpoints residual is admitted: Highlight `QuadPoints` rewrite public `bounding_box` when appearance is unset or `AP/N` stream Resources lack `ExtGState` (pdf.js `HighlightAnnotation` ignores such streams); `AP/N` stream with `ExtGState` keeps raw Rect. Fixtures prove no-AP uses `{10,10,100,80}`, AP without ExtGState uses `{10,10,100,80}`, and AP with ExtGState keeps `{200,200,300,300}`. Leaf-mutation count is frozen at 39. Underline/Squiggly/StrikeOut quadpoints breadth, appearance-stream content bbox derivation, line endings `LE`, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations text-markup quadpoints residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` text-markup quadpoints residual is admitted: Underline/Squiggly/StrikeOut without appearance rewrite public `bounding_box` from `QuadPoints` (pdf.js text-markup annotations). Squiggly uses bottom-strip `[minX, minY-2*dy, maxX, minY+2*dy]` with `dy=(maxY-minY)/6`; Underline/StrikeOut use the full axis-aligned quad union. Fixtures prove Underline `{20,20,120,90}`, Squiggly `{30,6.666...,130,53.333...}`, StrikeOut `{40,40,140,110}`. Leaf-mutation count is frozen at 39. Text-markup with appearance breadth, appearance-stream content bbox derivation, line endings `LE`, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations text-markup with-appearance residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` text-markup with-appearance residual is admitted: Underline/Squiggly/StrikeOut with usable `AP/N` appearance keep raw Rect public `bounding_box` and do not rewrite from `QuadPoints` (pdf.js text-markup annotations only synthesize when appearance is unset). Fixtures prove Underline keeps `{200,200,300,300}`, Squiggly keeps `{150,160,250,260}`, and StrikeOut keeps `{100,110,210,220}` despite non-intersecting QuadPoints. Leaf-mutation count is frozen at 39. Appearance-stream content bbox derivation, line endings `LE`, Widget named-state breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.


### read_pdf include_annotations stamp/caret/fileattachment residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` stamp/caret/fileattachment residual is admitted: Stamp, Caret, and FileAttachment markup annotations expose `subtype`/`contents`/`title`/`id`/`page` and keep raw Rect public `bounding_box` without appearance rewrite. Leaf-mutation count is frozen at 42. Appearance-stream content bbox derivation, FileAttachment payload surface, stamp icon-name breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.

### read_pdf include_annotations square/circle/widget residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` square/circle/widget residual is admitted: Square/Circle normalize inverted Rect public `bounding_box` like pdf.js `lookupNormalRect`; Widget annotations keep `subtype`/`contents`/`id`/`page`/`bounding_box` but do not project field `/T` as `title` (pdf.js `fieldName`, not `titleObj`). Leaf-mutation count is frozen at 41. Appearance-stream content bbox derivation, widget form-field parity, FreeText inverted breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.

### read_pdf include_annotations link-uri-normalize residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` link-uri-normalize residual is admitted: Link URI public `url` follows pdf.js `createValidAbsoluteUrl`/`href` normalization — bare domain gains trailing `/`, path URLs keep path, and `www.` hosts get `http://` default protocol when absolute. Leaf-mutation count is frozen at 39. Relative URL `docBaseUrl` breadth, `javascript:` URI, Launch file breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.

### read_pdf include_annotations freetext-rect residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` freetext-rect residual is admitted: FreeText public `bounding_box` follows pdf.js `lookupNormalRect` — inverted Rect is normalized, ordinary Rect is kept, and zero-width Rect is kept (not Popup-style nulling). Leaf-mutation count is frozen at 42. Appearance-stream content bbox derivation, richText RC, default appearance DA, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.

### read_pdf include_annotations link-uri-name residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` link-uri-name residual is admitted: Link URI Name becomes `/Name` like pdf.js `parseDestDictionary`; invalid absolute `javascript:` and relative paths keep the raw string via TS `url ?? unsafeUrl` fallback. Leaf-mutation count is frozen at 39. Relative URL `docBaseUrl` breadth, Launch file breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.

### read_pdf include_annotations link-aa-action residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` link-aa-action residual is admitted: when Link has no `/A` and no `/Dest`, pdf.js `parseDestDictionary` falls back to `/AA` additional-actions preferring `/D` then `/U`. AA/U URI projects `url`, AA/D GoTo projects `dest`, and present `/A` wins over `/AA`. Leaf-mutation count is frozen at 39. AA other triggers, relative URL `docBaseUrl` breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.

### read_pdf include_annotations link-aa-precedence residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_annotations` link-aa-precedence residual is admitted: pdf.js `parseDestDictionary` AA fallback ignores non-D/U triggers (`AA/E` alone projects no url), `/Dest` presence suppresses AA fallback, and `AA/D` is preferred over `AA/U` when both exist. Leaf-mutation count is frozen at 38. AA other triggers breadth, relative URL `docBaseUrl` breadth, and whole-product parity remain unclaimed; `include_annotations` remains `PARTIAL`.

### read_pdf include_form_fields form button-default-off residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form button-default-off residual is admitted: pdf.js `ButtonWidgetAnnotation` sets `defaultFieldValue` to `Off` when `DV` is null and `AP/N` is a named-state dict (checkbox/radiobutton). Without that `AP/N` shape, missing `DV` remains null. Fixtures prove checkbox AP → `default_value=Off`, radiobutton AP → `default_value=Off`, and checkbox no-AP → `default_value=null`. Leaf-mutation count is frozen at 45. Pushbutton AP default-off breadth, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.


### read_pdf include_form_fields form pushbutton-default-null residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form pushbutton-default-null residual is admitted: pdf.js pushbutton fields keep `defaultFieldValue` null when `DV` is missing, even with an `AP/N` named-state dict. Checkbox `AP/N` named-state missing `DV` still defaults to `Off`. Fixtures prove pushbutton AP → `default_value=null`, pushbutton no-AP → `default_value=null`, and checkbox AP regression → `default_value=Off`. Leaf-mutation count is frozen at 45. Pushbutton action/export breadth, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.


### read_pdf include_form_fields form checkbox-as-value residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form checkbox-as-value residual is admitted: pdf.js checkbox `_processCheckBox` sets `fieldValue` from the `AS` string when `AP/N` is a named-state dict. Without that AP shape, `V` remains authoritative and missing `DV` stays null. Fixtures prove `V=/Yes AS=/Off` → `value=Off`, `V=/Off AS=/Yes` → `value=Yes`, and no-AP `V=/Yes AS=/Off` → `value=Yes` with `default_value=null`. Leaf-mutation count is frozen at 45. Radio AS override breadth, export-value normalization breadth, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.


### read_pdf include_form_fields form checkbox-export-normalize residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form checkbox-export-normalize residual is admitted: pdf.js checkbox `_processCheckBox` builds `exportValues` from `AP/N` named-state keys and forces `fieldValue` to `Off` when the post-AS value is not an admitted export value. Fixtures prove invalid `AS=/Maybe` with `Off/Yes` export keys → `value=Off`, AP/N only-`Off` key with `AS=/Yes` → `value=Yes`, and valid `AS=/Off` → `value=Off`. Leaf-mutation count is frozen at 45. Radio export-normalize breadth, multi-export option breadth, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.



### read_pdf include_form_fields form radio-as-no-override residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form radio-as-no-override residual is admitted: pdf.js radio `_processRadioButton` does not apply checkbox `AS` overwrite or `exportValues` force-`Off`, so `V` remains authoritative for radiobutton widgets. Fixtures prove radiobutton `V=/Gold AS=/Silver` → `value=Gold`, radiobutton invalid `AS=/Maybe` → `value=Gold`, and checkbox `AS=/Off` regression → `value=Off`. Leaf-mutation count is frozen at 45. Radio parent/kids AS breadth, radio exportValues materialization, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.



### read_pdf include_form_fields form checkbox-multi-export residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form checkbox-multi-export residual is admitted: pdf.js checkbox `_processCheckBox` multi-export `AP/N` named-state options admit custom export values after `AS` overwrite, switch among admitted exports, and force `Off` for non-admitted `AS` values. Fixtures prove `AP/N={Foo,Off,Bar}` with `AS=/Foo` → `value=Foo`, `AS=/Bar` → `value=Bar`, and `AS=/Baz` → `value=Off`. Leaf-mutation count is frozen at 45. More-than-three export options breadth, radio multi-export breadth, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.



### read_pdf include_form_fields form checkbox-multi-export-many residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form checkbox-multi-export-many residual is admitted: pdf.js checkbox `_processCheckBox` multi-export `AP/N` with more than three named-state options admits selected custom exports after `AS` overwrite and forces `Off` for non-admitted `AS` values. Fixtures prove `AP/N={A,Off,B,C,D}` with `AS=/C` → `value=C`, `AS=/A` → `value=A`, and `AS=/Z` → `value=Off`. Leaf-mutation count is frozen at 45. Empty/single export-option breadth, radio multi-export breadth, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.



### read_pdf include_form_fields form checkbox-export-empty-single residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form checkbox-export-empty-single residual is admitted: pdf.js checkbox `_processCheckBox` empty and single-key `AP/N` exportValues construction admits `AS`-selected exports (empty `AP/N` synthesizes `Off/Yes`; single non-`Off` key synthesizes `Off+key`) and forces `Off` for non-admitted `AS` values. Fixtures prove empty `AP/N` + `AS=/Yes` → `value=Yes`, single `/Foo` + `AS=/Foo` → `value=Foo`, and single `/Foo` + `AS=/Bar` → `value=Off`. Leaf-mutation count is frozen at 45. Radio empty/single export breadth, malformed AP breadth, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.



### read_pdf include_form_fields form checkbox-malformed-ap residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form checkbox-malformed-ap residual is admitted: pdf.js checkbox `AS` overwrite and exportValues force-`Off` require `AP/N` named-state dict. Fixtures prove `AP` stream → `value=Yes`/`default_value=null`, `AP/N` stream → `value=Yes`/`default_value=null`, and named `AP/N` `AS=/Off` regression → `value=Off`/`default_value=Off`. Leaf-mutation count is frozen at 45. Radio malformed AP breadth, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.



### read_pdf include_form_fields form radio-malformed-ap residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form radio-malformed-ap residual is admitted: pdf.js radio default-`Off` requires `AP/N` named-state dict and `AS` does not override `V`. Fixtures prove radio `AP` stream → `value=Gold`/`default_value=null`, `AP/N` stream → `value=Gold`/`default_value=null`, and named `AP/N` with `AS=/Silver` → `value=Gold`/`default_value=Off`. Leaf-mutation count is frozen at 45. Radio parent/kids malformed AP breadth, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.



### read_pdf include_form_fields form radio-parent-kids-ap residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form radio-parent-kids-ap residual is admitted: pdf.js radio parent/kids inherit parent `V`; kid `AS` does not override; default-`Off` only when kid `AP/N` is a named-state dict. Fixtures prove kids with `AP` stream → `value=Gold`/`default_value=null`, kids with `AP/N` stream → `value=Gold`/`default_value=null`, and kids with named `AP/N` → `value=Gold`/`default_value=Off`, while the parent stub remains type/value null. Leaf-mutation count is frozen at 84. Deeper kids nesting breadth, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.


### read_pdf include_form_fields form radio-deeper-kids-ap residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form radio-deeper-kids-ap residual is admitted: pdf.js radio deeper kids nesting inherits parent `V` across an intermediate named group with Parent links; kid `AS` does not override; default-`Off` only when leaf `AP/N` is a named-state dict. Fixtures prove root `Plan` and intermediate `Plan.Opt` stubs remain type/value null, leaves with `AP` stream → `value=Gold`/`default_value=null`, leaves with `AP/N` stream → `value=Gold`/`default_value=null`, and leaves with named `AP/N` → `value=Gold`/`default_value=Off`. Leaf-mutation count is frozen at 90. Broken parent-chain intermediate breadth, malformed-field breadth, and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.

### read_pdf include_form_fields form radio-broken-parent-chain residual (frozen TS v3.0.14)

A frozen three-case public-stdio `read_pdf` `include_form_fields` form radio-broken-parent-chain residual is admitted: when a radio intermediate group lacks a Parent link to the radio root, pdf.js drops leaf widgets (no inheritable `FT`) and names the intermediate via Parent-chain construction only (`Opt`, not `Plan.Opt`). Root `Plan` stub remains across AP stream / AP/N stream / named AP/N variants. Rust form extraction matches by using Parent-chain name construction and Parent-chain `FT`/`V`/`Ff`/`DV` inheritance instead of Kids-path downward inheritance. Leaf-mutation count is frozen at 24. Malformed-field breadth and whole-product parity remain unclaimed; `include_form_fields` remains `PARTIAL`.

### search_pdf prefer_speed tools/list (post-3.0.14 additive surface)

Pure-Rust tools/list intentionally exposes `search_pdf.prefer_speed` as a post-3.0.14 additive boolean property matching the current TypeScript surface. Frozen v3.0.14 input-schema ranges/enums remain enforced; `prefer_speed` is not a detached v3.0.14 residual claim. `prefer_speed` remains `PARTIAL`; Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.


### search_pdf tesseract-tsv public-stdio subset (bounded)

A frozen two-case public-stdio `search_pdf` tesseract-tsv OCR subset is admitted over `v3014-visual-v1.pdf`: valid TSV level-5 words produce image-to-PDF `ocr_word` geometry on the search match, and malformed TSV soft-falls back to raw text without geometry. Leaf-mutation count is frozen at 34 with relocated fixture-root replay. `include_ocr_text_layer` remains `PARTIAL`; real tesseract binary health checks, selectable/OCR interleaving, URL single-fetch, and whole-product parity remain unclaimed. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.

### Cross-platform native optional packages (Stage B)

Optional native packages and a platform-scoped `bin/native/<platform>/` layout cover `darwin-arm64`, `darwin-x64`, `linux-arm64-gnu`, `linux-x64-gnu`, and `win32-x64-msvc`. Packages are publishable with binary-gated `prepublishOnly`, declared from the main package via `optionalDependencies`, and built/published through the multi-platform publish workflow under verified-candidate admission. Default npm entry remains TypeScript until sole-runtime cutover; clean five-platform registry install/runtime verification and TS retirement remain unclaimed.

Local pure-Rust launcher smoke (`bun run smoke:native-launcher`) verifies staged platform-path binary resolution and MCP initialize on the host. This is not registry install proof.

## Experimental pure-Rust npm library export

`@sylphx/pdf-reader-mcp/pure-rust` exports an opt-in client that resolves the
staged pure-Rust MCP server binary and calls `read_pdf` / `search_pdf` /
`pdf_evidence` over stdio. The default package export remains TypeScript
`dist/index.js`. Live product truth is sole-Rust 4.1.1 — see the capability matrix for current gates.
