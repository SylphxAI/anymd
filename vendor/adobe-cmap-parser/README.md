# anymd-adobe-cmap-parser

A fork of [`adobe-cmap-parser`](https://crates.io/crates/adobe-cmap-parser)
`v0.4.1` (MIT, (c) Jeff Muizelaar, see `LICENSE`) with a small correctness and
safety patch. It is published so that
[`anymd`](https://github.com/SylphxAI/anymd) can be installed with
`cargo install anymd`; crates.io does not accept patched path dependencies.
The library is still named `adobe_cmap_parser`. If you are not using anymd,
use the upstream crate.

## Why it is forked

The upstream crate panics with `bad length of hexstring` when a
`beginbfrange` destination is not 2 or 4 bytes long, and uses similar
`panic!`/`assert!`/indexing on other malformed input. Malformed or unusual
CMaps emitted by pdfTeX (for example 1-byte `beginbfrange` destinations and
6-byte ligature `beginbfchar` destinations) reachable from arbitrary PDFs
therefore crash the host process. Because the workspace `[profile.release]`
previously compiled with `panic = "abort"`, such a panic aborted the entire
MCP server.

## Patch delta vs upstream 0.4.1

- `get_unicode_map` is now effectively infallible on malformed input:
  - a parse failure returns an empty map instead of panicking;
  - an operator whose preceding count token is missing is skipped instead of
    indexing `lexed[i - 1]` out of bounds;
  - `beginbfrange` string destinations of odd byte length (not valid UTF-16BE,
    e.g. `<C5>`) are skipped instead of panicking;
  - even-length wide destinations (6/8 bytes, multi-code UTF-16BE) are
    preserved by advancing the full big-endian destination value instead of
    only handling 2/4 bytes;
  - array destinations are iterated with bounds checks and odd-length
    elements are skipped;
  - `beginbfchar` is skipped when the destination would not be valid UTF-16BE.
- `get_byte_mapping`:
  - a parse failure returns an empty mapping instead of panicking;
  - codespace ranges whose start/end have mismatched byte lengths are skipped
    instead of `assert!` panicking;
  - missing count/operand tokens bail out instead of indexing out of bounds.

The public API is unchanged, so `pdf-extract` still compiles unmodified; its
`.unwrap()` calls on these functions can no longer trigger a panic from
malformed CMaps.

## Verification

```bash
cargo test -p adobe-cmap-parser
```

Run from the pdf-reader-mcp workspace root (`vendor/adobe-cmap-parser` is
wired through `[patch.crates-io]` in `Cargo.toml`).
