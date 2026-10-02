# Binary size

The `linux-x64-gnu` release binary (what `npm i @sylphx/anymd` installs) was
35.0 MB. The release profile and one dependency feature set now bring it to
31.4 MB with byte-identical output on the 39-file benchmark corpus.

| Build (linux-x64-gnu, stripped) | Size | Output vs before |
|---|---|---|
| 8.4.0 main (thin LTO) | 35.05 MB | - |
| vendor image features trimmed | 34.09 MB | identical |
| + fat LTO (shipped) | 31.41 MB | identical |
| `--no-default-features` (no VLM OCR), same profile | 25.0 MB | n/a |

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

## Drafted: engines as on-demand components (not enabled)

The remaining large pieces are the VLM OCR engine (candle, ~6 MB with its
tokenizer and kernels; `--no-default-features` already builds without it) and
Qwen3-ASR (transcribe-cpp, ~1.5 MB+). Getting from 31 MB toward 20 MB means
moving the VLM engine out of the main binary:

1. Build `anymd-ocr-vlm` as a separate companion binary
   (`anymd-ocr-vlm-worker`) shipped as its own optional npm package
   `@sylphx/anymd-<platform>-ocr`, the way weights are fetched today.
2. The main binary spawns it over stdio JSON for `--ocr vlm`; if it is
   missing, `anymd setup ocr` downloads it next to the weights.
3. Default `npm i` is then ~25 MB (this table's no-default row) and `--ocr
   vlm` needs one extra explicit download on first use.

UX implication: today `--ocr vlm` needs `anymd setup ocr` (weights) only; the
split adds a second download of the engine, so the default experience changes
and that is a product decision. It is not done in this change. The existing
`ocr-vlm` Cargo feature already gives the main-binary half of that split.
