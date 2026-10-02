# Binary size

The `linux-x64-gnu` release binary (what `npm i @sylphx/anymd` installs) was
35.0 MB. The release profile and one dependency feature set brought it to
31.4 MB with byte-identical output on the 39-file benchmark corpus, and moving
the VLM OCR engine into a companion binary brings the default to 25.4 MB.

| Build (linux-x64-gnu, stripped) | Size | Output vs before |
|---|---|---|
| 8.4.0 main (thin LTO) | 35.05 MB | - |
| vendor image features trimmed | 34.09 MB | identical |
| + fat LTO | 31.41 MB | identical |
| companion split: default build, no VLM engine (shipped) | 25.38 MB | identical (the VLM path runs in the companion, 10.08 MB) |

## What is in the binary

By crate, `.text` of an unstripped build (cargo-bloat): std 2.9 MB,
candle (VLM OCR) ~2.0 MB plus oar-ocr-vl 0.6 MB and tokenizers 0.5 MB,
transcribe-cpp (Qwen3-ASR) 1.5 MB, anymd-core 1.1 MB, anymd 1.0 MB,
rmcp 1.0 MB, anymd-formats 0.6 MB, serde 1.1 MB, hayro (PDF render) ~1.5 MB,
image codecs ~1.6 MB, rustls+ring 0.7 MB. `.rodata`, `.eh_frame` and
`.gcc_except_table` add 6 MB.

## Done

- `vendor/oar-ocr-vl` pulled `image` with default features, which compiled in
  EXR, HDR, ICO, DDS, TGA, QOI, PNM and farbfeld decoders that nothing
  reaches (anymd feeds it PNG/JPEG/GIF/TIFF/BMP/WebP, the set the `anymd`
  crate already enables). It now asks for exactly that set. -0.95 MB.
- Release profile `lto = "fat"` (was thin). -2.7 MB, and PDF time did not
  regress (492-page NIST SP 800-53: 1.8-2.1 s before, 1.7-2.1 s after, same
  noise band). Build time is longer (about 6 min vs 4.5 on 4 cores).

## Tried, not adopted

- `opt-level = "s"` on the transport and TLS crates (rmcp, axum, hyper,
  rustls, ureq, schemars): only -0.3 MB and measured slightly slower on the
  492-page PDF in one run; not worth it.
- `opt-level = "s"` also on candle, tokenizers and gemm: -1.1 MB more, but
  VLM OCR speed cannot be measured on a shared CI box (59-254 s per page), so
  it is not adopted without a model benchmark.
- `panic = "abort"`: not safe. The PDF pipeline wraps parsing in
  `catch_unwind` to survive malformed files.

## Done: the VLM engine is a companion binary

The in-process VLM engine (candle, oar-ocr-vl and the tokenizer, ~6 MB of
`.text` plus kernels) now lives in its own executable, `anymd-ocr-vlm`. The
default `anymd` build has the `ocr-vlm` Cargo feature off, so `npm i`, `pip
install`, the container image and `cargo install anymd` get the ~25 MB binary.

- `anymd setup ocr` downloads the platform's `anymd-ocr-vlm-<platform>` asset
  from this version's GitHub release, checks it against the release's
  `anymd-ocr-vlm-SHA256SUMS`, and installs it next to the weights
  (`<cache>/models/docvlm-v1/`), with a version stamp. Then it fetches the
  weights as before. The engine goes first because it is small and fails
  fastest.
- `--ocr vlm`, or `auto` once weights and engine are installed, runs the
  existing `__ocr-vlm-worker` protocol against the companion instead of the main
  binary: page image path and token cap in argv, one JSON evidence document on
  stdout, same supervision, deadline and size caps. The worker code is the same
  function, so the Markdown is the same.
- Without the setup, VLM OCR answers `VLM OCR needs a one-time setup: run
  `anymd setup ocr``. Over MCP this is a normal tool result, like the Pro notice.
  A companion from a different anymd version counts as not installed.
- Tesseract OCR and every other feature are unchanged and need no setup.
- `cargo build -p anymd --features ocr-vlm` links the engine in (and builds
  the companion); such a build needs no companion file.
- It is not a separate npm package: the release uploads the companions as
  GitHub release assets, so the npm `optionalDependencies` layout is untouched.

UX change: `--ocr vlm` already needed the explicit `anymd setup ocr` step for
about 2 GB of weights; the engine (about 20 MB) comes in the same step, so
there is no new step.

Qwen3-ASR (transcribe-cpp, ~1.5 MB+) stays in the main binary.

Measured on linux-x64-gnu with the shipped profile: default `anymd` 25.38 MB,
the `anymd-ocr-vlm` companion 10.08 MB, the old all-in-one build
(`--features ocr-vlm`) 31.41 MB. Cold start is unchanged: `anymd version`
median 2.5 ms vs 2.9 ms all-in-one, `anymd sample.pdf` 5.7 ms vs 5.3 ms (30
runs each). The companion adds its own load time only to a VLM request, where
model loading dominates. The worker is the same function in the
all-in-one binary and the companion.
