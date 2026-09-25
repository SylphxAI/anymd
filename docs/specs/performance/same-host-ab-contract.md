# Same-host TS 3.0.14 vs Rust candidate A/B contract

> Historical: a pdf-reader-mcp release record, kept as written. The `verification/` files it names are no longer in this repository; anymd releases are described in [PUBLISH.md](../../PUBLISH.md).

Status: rebuilt executable suite (formal marketing claims still require independent review)

## Purpose

Prove a material, reproducible performance advantage for sole-Rust versus
immutable `@sylphx/pdf-reader-mcp@3.0.14` on the same host, without semantic
regressions.

## Modes (must not be confused)

1. **startup_inclusive**  
   Each sample: spawn process → initialize → task → kill.  
   This measures end-to-end cold/start path. It is **not** steady-state warm read latency.

2. **persistent_warm**  
   One long-lived process per engine; discard warm-up call; time only subsequent `tools/call`.  
   This is the primary formal warm-latency mode.

## Hard rules

1. Same machine, corpus, inputs, configuration, task semantics.
2. Semantic task outcomes must pass before timing is admitted.
3. Interleave/randomize engine order.
4. Separate startup_inclusive and persistent_warm.
5. Record median/p95; retain raw samples under `verification/perf/raw/<runId>/`.
6. Bind host/toolchain/binary digests.
7. Capability-specific tasks for labeled classes (not full_text-only for every class).
8. Prefer registry-installed exact published natives when admitting release claims (`MCP_PDF_PERF_RUST_FROM_REGISTRY=1`).
9. Historical cross-run numbers are not marketing claims.
10. Fail closed unless suite `status=admissible_pass`.

## Semantic gate

- Parse tool JSON payload (not raw transport length alone).
- Normalize text for comparison/hashing.
- Task-specific floors:
  - full_text: non-empty normalized text or explicit success
  - tables: table count > 0 or non-empty text
  - structure: document map nodes > 0 or non-empty text
  - geometry: success / text-layer markers / non-empty text
  - search: matches > 0

Exact cross-engine digest agreement is reported; capability-first may allow equivalent non-identical representations when both succeed.

## Fixture / suite admission

### Fixture `fixture_pass`

Requires **persistent_warm**:

- semantic pass rate 1.0 both engines
- warm iterations ≥ 5
- warm median speedup (TS/Rust) ≥ 1.5
- rust p95 / ts p95 ≤ 1.15

### Suite `admissible_pass`

- all required classes present and persistent_warm `fixture_pass`
- min persistent_warm speedup ≥ 1.5
- no failed runs

## Commands

```bash
bun run build:rust
# local binary diagnostic
bun run perf:same-host-ab-suite

# formal-ish registry binary path
MCP_PDF_PERF_RUST_FROM_REGISTRY=1 bun run perf:same-host-ab-suite
bun run perf:same-host-ab-suite -- --require-admissible
```

## Marketing claims

Require:

1. suite `admissible_pass`
2. independent `performanceClaimsAuthorized=true`
3. published honest report with method bounds
