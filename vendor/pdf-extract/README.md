# anymd-pdf-extract

A fork of [`pdf-extract`](https://crates.io/crates/pdf-extract) `0.12.1`
(MIT, (c) Jeff Muizelaar, see `LICENSE`), published so that
[`anymd`](https://github.com/SylphxAI/anymd) can be installed with
`cargo install anymd`; crates.io does not accept patched path dependencies.
The library is still named `pdf_extract`. If you are not using anymd, use the
upstream crate.

Changes from upstream 0.12.1:

- Form XObjects run with the current transformation matrix, so figure text
  lands where it is drawn instead of at the page origin.
- Image XObjects are no longer parsed as content streams.
- Every path-painting operator (`f*`, `B`, `b`, `s`) reaches the output device,
  so table rules are reported.
- It depends on `anymd-adobe-cmap-parser`, the fork that does not panic on
  malformed CMaps.

The upstream README follows.

## pdf-extract
[![Build Status](https://github.com/jrmuizel/pdf-extract/actions/workflows/rust.yml/badge.svg)](https://github.com/jrmuizel/pdf-extract/actions)
[![crates.io](https://img.shields.io/crates/v/pdf-extract.svg)](https://crates.io/crates/pdf-extract)
[![Documentation](https://docs.rs/pdf-extract/badge.svg)](https://docs.rs/pdf-extract)

A rust library to extract content from PDF files.

```rust
let bytes = std::fs::read("tests/docs/simple.pdf").unwrap();
let out = pdf_extract::extract_text_from_mem(&bytes).unwrap();
assert!(out.contains("This is a small demonstration"));
```

## See also

- https://github.com/elacin/PDFExtract/
- https://github.com/euske/pdfminer / https://github.com/pdfminer/pdfminer.six
- https://gitlab.com/crossref/pdfextract
- https://github.com/VikParuchuri/marker
- https://github.com/kermitt2/pdfalto used by [grobid](https://github.com/kermitt2/grobid/)
- https://github.com/opendatalab/MinerU (uses PyMuPDF and pdfminer.six)

### Not PDF specific
- https://github.com/Layout-Parser/layout-parser
