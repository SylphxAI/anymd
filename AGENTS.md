# anymd — agent notes

anymd (formerly Citra and pdf-reader-mcp) turns any file into Markdown for AI
agents. It is one Rust binary (`crates/anymd`) that is both the MCP server and
the CLI, published to npm as `@sylphx/anymd`. Project facts are in `PROJECT.md`;
publishing is in `docs/PUBLISH.md`.

## Layout

- `crates/anymd`: the binary: MCP server on rmcp (stdio and Streamable HTTP),
  CLI, `setup` (client registration from mcp-kit) and `version`.
- `crates/anymd-core`, `anymd-pdf`, `anymd-formats`: conversion; `anymd-wasm`:
  the docs playground build.
- `packages/anymd`: the npm package; `bin/anymd.js` is mcp-kit's launcher, which
  runs the matching `packages/npm/<platform>` binary. `packages/aliases/*` are
  `@sylphx/citra` and `@sylphx/pdf-reader-mcp`, which run the same launcher.
- `test/`: TypeScript tests that spawn the cargo-built binary over MCP.

## Boundary hazards

- Local-first privacy: do not upload documents or call remote providers unless
  the caller explicitly selects a remote provider adapter.
- No hosted auth, billing, storage, tenancy or customer-account state here.
- Optional OCR, vision and region providers stay behind typed adapters that
  fail closed.
- Public MCP schemas are contracts: version and regression-test option and
  output shapes. The Rust server is their only authority.
- Keep page, region and source provenance on extraction and analysis outputs.
- Publishing happens only in `release.yml` on `main`, because npm trusted
  publishing accepts that workflow alone; a version bump PR is the release.
- Never commit secrets, private documents or customer data.

## Commands

```bash
bun install --frozen-lockfile
bun run check             # Biome
bun run check:versions    # every manifest carries one version
bun run typecheck
bun run build             # cargo build --release -p anymd
bun run test:rust
bun run test:cov          # needs the built binary
bun run docs:build
bun run check:github-actions
```

`ANYMD_BIN=/path/to/anymd` points the tests and the npm launcher at a binary
built elsewhere (for example under `CARGO_TARGET_DIR`).

## Releasing

`bun scripts/set-version.ts X.Y.Z`, `cargo update -w`, and a `## X.Y.Z` section
in `CHANGELOG.md`, in one pull request. Merging publishes it through the shared
mcp-kit workflow; the post-release check is `npx -y @sylphx/anymd@X.Y.Z version`.

## Reporting

Report each layer separately: local diff, merged source, published npm
version, MCP Registry entry.
