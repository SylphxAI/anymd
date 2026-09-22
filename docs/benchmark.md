---
layout: doc

title: Benchmark Proof
description: Reproducible release evidence for PDF Reader MCP V3 — performance, quality, corpus, provider, and SOTA release gate results.
---

# Benchmark Proof

Every PDF Reader MCP release is gated by reproducible, machine-checked benchmarks.
The numbers below come from the checked-in release artifacts in
`benchmark-artifacts/` and are produced by `bun run benchmark:release-gate`.

## SOTA Release Gate

| Metric | Result |
| --- | --- |
| Total checks | 39 |
| Passing checks | 39 |
| Failed checks | 0 |
| Gate status | **Passed** ✅ |

The release gate enforces deterministic quality coverage, corpus benchmark
completeness, provider certification, package smoke, and cross-artifact
consistency. It exits non-zero if any check fails.

## Quality Benchmark

| Metric | Result |
| --- | --- |
| Score | 1.0 (perfect) |
| Deterministic quality checks | 69 / 69 passing ✅ |
| Final-bar coverage | 5 / 5 deterministic + 4 / 4 provider-required |

Quality checks cover document-signal detection, reading-order fidelity,
scanned-PDF routing, text-layer structure, table extraction, trust-report
signals, accessibility reporting, and provider manifest scoring — all against
deterministic synthetic and runtime-generated fixtures.

## Performance Benchmark

Measured on the checked-in sample fixture (`test/fixtures/sample.pdf`) with
20 iterations after 3 warmup iterations. Results are machine- and
fixture-specific; reproduce with `bun run benchmark`.

| Scenario | Average | Min | Max |
| --- | --- | --- | --- |
| Metadata + page count | 1.1 ms | 0.6 ms | 2.6 ms |
| Full text extraction | 16.1 ms | 10.3 ms | 21.1 ms |
| Single-page text | 13.7 ms | 8.3 ms | 24.4 ms |
| structured document result | 27.2 ms | 20.1 ms | 39.0 ms |

The **structured document result** scenario includes document map, text layer,
semantic AST, trust report, accessibility report, citation chunks, layout
diagnostics, tables, and trust/accessibility routing — all in one call.

## Reproduce

```bash
# Performance benchmark only
bun run benchmark

# All benchmark profiles
MCP_PDF_BENCHMARK_OUTPUT_DIR=./benchmark-artifacts bun run benchmark:all

# Provider-certified release artifacts
MCP_PDF_BENCHMARK_OUTPUT_DIR=./benchmark-artifacts MCP_PDF_PROVIDER_BENCHMARK_REQUIRED=true bun run benchmark:all
bun run benchmark:release-artifacts

# Run the SOTA release gate
MCP_PDF_BENCHMARK_OUTPUT_DIR=./benchmark-artifacts bun run benchmark:release-gate
```

All benchmark scripts print JSON to stdout and can write formatted artifacts
with `--output <path>` or `--output-dir <dir>`.

## What This Proves

- **Quality is deterministic**: 69/69 synthetic quality checks pass on every run,
  not just on the release machine.
- **Performance is bounded**: full structured document result extraction completes in
  under 40 ms on a small fixture, with the metadata path under 3 ms.
- **Release is gated**: no version ships unless all 39 release-gate checks pass.
- **Providers are certified**: OCR, region analysis, and visual enrichment
  providers are tested against deterministic manifests and crop substrates.
- **Package is smoke-tested**: the published tarball is installed and exercised
  before any release.

## Benchmark Artifacts

| Artifact | What it gates |
| --- | --- |
| `pdf_sota_release_gate.json` | Cross-artifact release gate (39 checks) |
| `pdf_quality_benchmark.json` | Deterministic quality (69 checks, score 1.0) |
| `pdf_performance_benchmark.json` | Latency across 4 fixed scenarios |
| `pdf_corpus_benchmark.json` | Corpus-style PDF intelligence assertions |
| `pdf_provider_benchmark.json` | Provider profiles (4 certified) |
| `pdf_provider_manifest_benchmark.json` | Table, formula, chart, figure, image scoring |
| `pdf_provider_manifest_crop_benchmark.json` | Deterministic crop-substrate proof |
