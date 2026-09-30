# anymd

anymd turns any file into Markdown for AI agents: one Rust binary (`crates/anymd`) that is both the MCP server and the CLI, published to npm as `@sylphx/anymd`. Goal, boundaries and target metrics are in [docs/vision.md](docs/vision.md); the release process is in [docs/PUBLISH.md](docs/PUBLISH.md). Company standards: <https://github.com/SylphxAI/owner/tree/main/standards>.

## Layout

- `crates/anymd`: MCP server on rmcp (stdio and Streamable HTTP), the CLI, `setup` and `version`.
- `crates/anymd-core`, `anymd-pdf`, `anymd-formats`: conversion, published to crates.io. `anymd-wasm`: the docs playground.
- `packages/anymd`: the npm launcher, which runs the matching `packages/npm/<platform>` binary. `packages/aliases/*` are the `@sylphx/citra` and `@sylphx/pdf-reader-mcp` aliases.
- `test/`: TypeScript tests that spawn the built binary over MCP. `bench/`: AgentDocBench. `brand/`: brand masters.

## Hard lines

- Documents stay local. Uploading them or calling a remote provider happens only when the caller selects one, because privacy is the product.
- No hosted auth, billing, storage or tenancy state here, so the package stays a plain local tool.
- Public MCP tool schemas are contracts. Version them and regression-test option and output shapes; the Rust server is their only authority.
- Extraction and analysis outputs keep page, region and source provenance, so agents can cite them.
- OCR, vision and region providers sit behind typed adapters that fail closed.
- Publish only through `release.yml` on `main`, because npm trusted publishing accepts that workflow alone. A version-bump pull request is the release.
- Commit no secrets, private documents or customer data.

## How a result is judged

- Pull request checks (`.github/workflows/ci.yml`): `Validate Code Quality`, `security:secrets`, `Plain language`, `Identifiers`. Locally: `bun run check`, `bun run check:versions`, `bun run check:copy`, `bun run typecheck`, `bun run build`, `bun run test:rust`, `bun run test:cov`, `bun run docs:build`, `bun run check:github-actions`.
- Accuracy and speed: AgentDocBench (`bench/`, [Benchmark workflow](.github/workflows/benchmark.yml)). A converter change is judged by its score, reading order, table F1, time and tokens against the committed results.
- Public copy comes from `product.json` and `bench/leaderboard.py`: edit those and run `bun scripts/render-copy.ts`; `bun run check:copy` fails on drift. Brand files come from `brand/`: run `python3 brand/build.py` after editing.
- After a release: `npx -y @sylphx/anymd@X.Y.Z version`.
- `ANYMD_BIN=/path/to/anymd` points the tests and the launcher at a binary built elsewhere.
