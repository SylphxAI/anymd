# ADR-0005 — Capability-first semantic compatibility replaces exact PDF.js output parity

- **Status:** Accepted
- **Date:** 2026-07-22
- **Relates to:** ADR-PROPOSED fleet Rust north star, `docs/specs/pure-rust-capability-matrix.json`, `docs/specs/temporary-rust-migration-fences.md`, `docs/specs/capability-first-admission-contract.md`
- **Change class:** `required-now` for Rust admission and release bar

## Context

The pure-Rust cutover program admitted many bounded TS v3.0.14 public-stdio
differential residuals. That program proved useful for:

- locking interface/schema contracts;
- discovering coverage gaps;
- preserving security/resource fail-closed behavior;
- preventing accidental publish while Rust is incomplete.

It also produced a long tail of exact-output residual PRs that mostly re-encode
PDF.js implementation quirks:

- annotation appearance precedence edge cases;
- malformed object fallbacks;
- checkbox/radio `/AP` `/AS` micro-details;
- UTF-16 surrogate and locale lowercase quirks;
- bounding-box rounding and null/missing/empty representation differences.

Exact JSON equality with PDF.js is no longer a sustainable whole-product
admission bar. It binds Rust architecture to PDF.js object-model accidents,
creates near-infinite residual PR volume, and does not necessarily improve agent
document intelligence.

## Decision

### 1. Admission bar is capability-first semantic compatibility

Rust may replace TypeScript when it provides **equivalent or better PDF
intelligence capability** for agent tasks, not when it reproduces PDF.js output
byte-for-byte.

Three compatibility layers are mandatory and distinct:

1. **Interface compatibility** — strict/exact
   - MCP tool names
   - input required/optional rules
   - JSON Schema field types
   - transport and JSON-RPC envelope
   - error classification
   - page numbering convention
   - deterministic/stable system IDs where claimed
   - capability availability and fail-closed behavior

2. **Semantic capability equivalence** — primary release standard
   - same user tasks must be supportable
   - table/search/AST/map/OCR/visual/form/annotation results must preserve task-relevant meaning
   - provenance/page/location evidence must be usable
   - representation differences (whitespace, IDs, reasonable bbox deltas, ordering within equivalence class) may be accepted when classified

3. **Quality parity** — task-eval gated
   - same corpus, same agent tasks, Rust evidence quality not below calibrated TS baseline
   - thresholds come from corpus measurement, not invented percentages

### 2. TS 3.0.14 exact differentials are demoted, not deleted

TS 3.0.14 remains valuable as:

- regression baseline;
- coverage discovery tool;
- compatibility reference;
- mismatch classification source.

It is **not** the sole whole-product oracle.

Existing exact residual families remain frozen evidence. New work must not
expand exact residual volume unless the mismatch is classified as:

- contract break;
- semantic regression;
- security/resource fail-closed gap.

### 3. Mismatch taxonomy is mandatory

Every TS-vs-Rust mismatch must be classified as one of:

1. `contract_break` — must fix
2. `semantic_regression` — must fix
3. `equivalent_representation` — accept and document
4. `rust_improvement` — accept and document
5. `pdfjs_implementation_non_goal` — stop chasing
6. `unknown` — send to task-eval, do not hand-wave

### 4. Non-claims require reclassification before unfreeze

The historical `explicitlyNotClaimed` list mixes true capability gaps with
PDF.js non-goals. Before `publishFreeze=false` or `dropInFor3014=true`:

1. complete reclassification of open non-claims into the taxonomy in
   `docs/specs/nonclaim-reclassification-ledger.json`;
2. prove only remaining `blocking_capability_gap` and unresolved `quality_risk`
   items are closed or explicitly accepted with task-eval evidence;
3. keep security/resource bounds executable and fail-closed.

### 5. Publish freeze remains until the new bar is met

This ADR does **not** unfreeze publish and does **not** claim drop-in.

`productTruth.dropInFor3014=false` and `productTruth.publishFreeze=true` remain
until:

- interface contracts are green;
- core capabilities have executable semantic contracts;
- agent-task corpus acceptance is green against calibrated thresholds;
- five-platform clean native install/runtime is proven;
- whole-product independent review of one exact candidate passes;
- TS retirement/rollback plan is complete;
- registry/install readback is complete.

### 6. Architecture ownership

Canonical document model and capability modules are owned by Rust
(`anymd-core` + MCP server boundary), not reverse-engineered from PDF.js
response shapes.

```text
PDF parse/render adapters
        ↓
Canonical Rust Document Model
        ↓
Capability modules
  text / table / search / OCR / image /
  structure / forms / annotations / evidence
        ↓
Stable MCP semantic projection
```

External engines remain optional adapters. Do not replace the pure-Rust default
runtime without measured corpus failure of the current stack.

## Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| Continue exact PDF.js output parity as sole bar | Infinite residual tail; architecture capture by PDF.js quirks |
| Immediately treat all PARTIAL as PASS / unfreeze | Dishonest; mixes representation non-goals with real gaps |
| Replace stack with commercial PDF SDK / PDFium / MuPDF now | Licensing, native packaging, and pure-Rust posture costs without measured necessity |
| Word/DOCX library as PDF path | Wrong format boundary |

## Consequences

- New residual PRs that only re-encode PDF.js representation quirks are out of
  scope unless they prove a contract/semantic/security break.
- Matrix legend and admission docs describe capability/semantic status, not
  “identical to PDF.js JSON”.
- Exact differential suites remain as frozen regression assets.
- A nonclaim reclassification ledger becomes part of release evidence.
- Agent-task corpus and semantic contracts become the path to unfreeze.

## Validation

- `bun run check:pure-rust-matrix`
- matrix keeps `dropInFor3014=false`, `publishFreeze=true`
- admission contract + reclassification ledger present and checker-enforced
- no publish-path change in this ADR candidate
