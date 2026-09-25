# Migration

anymd was called **pdf-reader-mcp**, then **Citra**. Existing setups keep working; this page lists what changed and how to move to the new names.

## Packages

| Before | Now |
|---|---|
| `npx -y @sylphx/pdf-reader-mcp` (bin `pdf-reader-mcp`) | `npx -y @sylphx/anymd` (bin `anymd`) |
| `npx -y @sylphx/citra` (bin `citra`) | `npx -y @sylphx/anymd` |
| MCP registry `io.github.SylphxAI/pdf-reader-mcp`, `io.github.SylphxAI/citra` | `io.github.SylphxAI/anymd` |
| `@sylphx/citra-<platform>` native packages | `@sylphx/anymd-<platform>` |
| `CITRA_RUST_BIN` | `ANYMD_BIN` (`ANYMD_RUST_BIN` still works) |
| SDK `@sylphx/anymd/sdk` (class `Anymd` or `Citra`) | Removed in 8.0.0. Run the `anymd` CLI, or connect to `anymd mcp` with an MCP client |

`@sylphx/pdf-reader-mcp` and `@sylphx/citra` are published at the same version as thin aliases that run anymd, so old configs need no change. For new installs, and to get a shorter server name in your client, switch the command to `npx -y @sylphx/anymd`.

`--allow-dir` and `MCP_PDF_ALLOWED_DIRS` are unchanged.

## Tools

| Before | Now |
|---|---|
| `read_pdf` | `read` |
| `search_pdf` | `search` |
| `pdf_evidence` | `inspect` (same operations) |
| `pdf_compare` | `inspect` with `operation: "compare"` |

The old names still work for this major version but no longer appear in `tools/list`, so agents discover the new ones.

The new tools take simpler arguments:

```json
// before
{ "sources": [{ "path": "report.pdf", "pages": "1-5" }] }
// now
{ "source": "report.pdf", "pages": "1-5" }
```

`search` takes plain strings in `sources` (files, folders, or URLs) and searches every supported format, not just PDFs.

## Markdown by default

`read` and `search` answer in compact Markdown: `<!-- page N -->` anchors, a small front-matter header, pipe tables, and a `max_tokens` budget (default 20000) with a cursor. On *Attention Is All You Need*, that is 42 KB instead of a 2 MB JSON envelope.

## Structured JSON

If you relied on the JSON result (document map, elements with bounding boxes, tables with geometry, trust and accessibility reports), it is still there:

- **Now:** `inspect` with `operation: "structure"` and a `profile` of `fast` (default), `quality`, or `research`:

  ```json
  { "operation": "structure", "sources": [{ "path": "report.pdf" }], "profile": "research" }
  ```

- **Legacy `read_pdf`:** passing `profile` (`fast`, `quality`, `research`) or any `include_*` flag still returns the JSON result. A plain `read_pdf` call with only `sources` now returns Markdown.
- **Legacy `search_pdf`:** pass `detail: true` for the JSON result with match geometry and provenance instead of the one-line-per-hit list.
