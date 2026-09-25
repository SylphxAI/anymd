# Installed product footprint comparison

> Historical: a pdf-reader-mcp release record, kept as written. The `verification/` files it names are no longer in this repository; anymd releases are described in [PUBLISH.md](../../PUBLISH.md).

Status: methodology + representative measurements for product messaging  
Date: 2026-07-24

## Do not compare

| Misleading comparison | Why it is wrong |
| --- | --- |
| TS main tarball (~90–100 KB) vs Rust **executable** (~20–25 MB) | Compares a package manifest wrapper to a full engine binary |
| “JS is smaller because no .so/.exe” | Ignores `node_modules` closure (PDF.js, SDK, transitive deps) |
| Sum of all five native packages | Users install **one** platform optional package |

## Compare this instead

For a **clean install on one host**:

1. Main package unpacked size  
2. Downloaded tarball bytes for packages actually installed  
3. Full `node_modules` installed footprint (disk usage)  
4. File count  
5. Production dependency graph shape  
6. Matching native binary size (Rust only; one platform)

## Representative registry metadata (public npm)

| Item | TS `3.0.14` | Rust `4.0.2` |
| --- | ---: | ---: |
| Main package `dist.unpackedSize` | ~402,169 bytes | ~72,415 bytes |
| Production `dependencies` | PDF.js, MCP TS SDK, Zod, PNG, … | `{}` |
| Platform engine | In-process JS + PDF.js workers | One optional native package |

## Representative clean-install findings (audit)

Independent post-publish audits measured on real hosts (values vary by OS/arch):

| Metric | TS `3.0.14` | Rust `4.0.2` | Notes |
| --- | ---: | ---: | --- |
| Actual downloaded tarballs | ~25 MB class | ~8 MB class | Platform-dependent |
| Full installed footprint | ~80–90 MiB class | ~20 MiB class | Includes one native binary on Rust |
| Installed files | thousands | tens | Rust install graph is tiny |
| Native executable | n/a | ~16–25 MB before/after strip work | Expected for a self-contained engine |

**Product truth:** Sole-Rust 4.x is a **cleaner, smaller install** than TS 3.0.14 on measured hosts, even though the native binary alone is multi-megabyte.


## Measured clean install — linux-x64 (2026-07-24)

Host: `linux x64`, Node `v24.3.0`, npm install of exact versions.

| Metric | TS `3.0.14` | Rust `4.0.2` | Ratio (TS/Rust) |
| --- | ---: | ---: | ---: |
| Main package on disk | 402,556 B | 76,786 B | ~5.2× |
| Full `node_modules` | 86,288,102 B (~82.3 MiB) | 25,553,107 B (~24.4 MiB) | **~3.4×** |
| Installed files | 4,101 | 20 | **~205×** |
| Production deps | PDF.js + MCP SDK + Zod + PNG | `{}` + `pdf-reader-mcp-linux-x64-gnu` | cleaner graph |

Evidence:

- `verification/footprint/3.0.14-linux-x64.json`
- `verification/footprint/4.0.2-linux-x64.json`

Release binary optimization note: with workspace `[profile.release]` (`strip=symbols`, thin LTO, `codegen-units=1`), local linux-x64 `pdf-reader-mcp-server` built to **~15.0 MiB**. Crashing on malformed PDFs is a real bug class for an MCP server, so the release profile is deliberately built **panic-unwind** (worker-thread panics degrade to a failed request instead of aborting the whole process; see issue #608). Unwinding adds roughly **+1.3 MiB (~+9%)** to the server binary versus `panic=abort` — the installed-footprint story vs TS remains intact. Measured on the issue-608 fix tree (linux-x64, `strip=symbols`, thin LTO): `panic=abort` ~15.01 MiB vs `panic=unwind` ~16.36 MiB. Published 4.0.2 natives may still be pre-optimization until the next binary rebuild/publish.

## How to reproduce

```bash
# metadata
npm view @sylphx/pdf-reader-mcp@3.0.14 dist.unpackedSize dependencies
npm view @sylphx/pdf-reader-mcp@4.0.2 dist.unpackedSize dependencies

# clean install footprint (example)
bun scripts/perf/measure-installed-footprint.ts --version=4.0.2
bun scripts/perf/measure-installed-footprint.ts --version=3.0.14
```

Record host triple, npm/node versions, and exact package versions with every public claim.

## Messaging rules

Allowed:

- “Cleaner install graph: empty production dependencies + one platform native binary”
- “Measured installed footprint smaller than TS 3.0.14 on host X”
- “Native binary is multi-megabyte because it contains the engine”

Forbidden:

- “Rust is 250× larger” from wrapper-vs-binary
- Claiming multi-host size guarantees from one host
- Summing all native packages into “install size”
