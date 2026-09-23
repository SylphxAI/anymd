# Performance

Fast, and honest about how that was measured. Every number here is
**method-bounded** — the bounds are part of the claim, not fine print.

## Headline

| | |
| --- | --- |
| **≥ 10.4×** | median warm `read_pdf` latency vs the TypeScript engine — same host (linux-x64), 8 required fixture classes, median of class speedups ~15.4× |
| **~3.4× smaller** | clean install — 82.3 MiB → 24.4 MiB of `node_modules` |
| **20 files** | installed on disk vs 4,101 |
| **0** | production JS dependencies — one native binary per platform |

## What "10.4×" means precisely

Two modes, never collapsed into one number:

| Mode | What it measures |
| --- | --- |
| `startup_inclusive` | spawn the process + `initialize` + **one** task, per sample |
| `persistent_warm` | long-lived MCP server; time **only** `tools/call` after warm-up |

The headline figure is `persistent_warm` — and `persistent_warm` includes the
process-local cache for **identical** local re-reads (same path + mtime/size +
request fingerprint). **The first request in a process still pays full parse
cost.** That is the boundary; please keep it attached to any quote of this
number.

## The measurement

- **Host class:** linux-x64 (same host for both sides of the A/B)
- **Baseline:** the historical TypeScript engine (`3.0.14`)
- **Candidate:** the sole-Rust release (`4.1.0`) installed from the registry
  (`rustFromRegistry: true`), with its platform native binary
- **Task family:** local `read_pdf` across the **eight required fixture classes**
- **Result:** every required class `fixture_pass`; min warm median speedup
  **10.37×**, median of class speedups **15.38×**

Evidence: [`verification/pdf-reader-same-host-ab-suite-4.1.0-registry.json`](https://github.com/SylphxAI/citra/blob/main/verification/pdf-reader-same-host-ab-suite-4.1.0-registry.json) ·
report: [4.1.0 same-host performance report](https://github.com/SylphxAI/citra/blob/main/docs/specs/performance/4.1.0-same-host-performance-report.md)

## Install footprint

Measured **clean installs** on linux-x64 — not "JS wrapper tarball vs native
executable":

| | Historical TS `3.0.14` | Sole-Rust lineage |
| --- | ---: | ---: |
| Main package on disk | ~403 KB | ~77 KB |
| Full `node_modules` | ~82.3 MiB | **~24.4 MiB** |
| Installed files | 4,101 | **20** |
| Production npm dependencies | PDF.js + MCP TS SDK + more | **none** + one platform native |

The native binary is multi-megabyte because it **is** the PDF engine. That is
expected — and still a cleaner install than shipping PDF.js and a large JS tree.

## What we do not claim

The [4.1.0 claims policy](https://github.com/SylphxAI/citra/blob/main/docs/specs/performance/4.1.0-performance-claims-policy.md)
forbids:

- collapsing modes into one unqualified "Nx faster"
- multi-host extrapolation from one host
- first-request latency presented as warm-cache latency
- memory/RSS marketing without raw samples
- OCR or external provider I/O speed

If you see those claims attributed to Citra, they are not ours.

## Why it is fast

The production PDF engine is **native Rust**, behind a thin Node launcher. The
JavaScript layer does no PDF processing at all. See [Why Rust](/performance/why-rust).

## Reproduce it

The suite is in the repository and the claims policy names the evidence files it
requires. If you re-run it on different hardware, publish the host class and the
mode with your number — the bounds travel with the claim.

## Next

- [Why Rust](/performance/why-rust)
- [Benchmark proof](/benchmark)
- [API reference](/api/)
