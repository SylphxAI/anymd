# Contributing to anymd

## How to Contribute

1. **Reporting Issues:** Search [existing issues](https://github.com/SylphxAI/anymd/issues) before opening a [bug report or feature request](https://github.com/SylphxAI/anymd/issues/new/choose).
   - Include the anymd version, command or MCP tool arguments, and a small synthetic or public document that reproduces the problem.
   - Describe the expected Markdown and the actual result. Redact private documents, personal paths, tokens and client configuration before sharing logs.
   - Explain the use case and a testable outcome for feature requests.
   - Use [Discussions](https://github.com/SylphxAI/anymd/discussions) for questions and ideas; report vulnerabilities through [SECURITY.md](SECURITY.md).

2. **Submitting Pull Requests:**
   - Fork the repository.
   - Create a new branch for your change.
   - Make your changes, adhering to the project's coding style and guidelines.
   - Add tests for your changes and ensure all tests pass.
   - Ensure your commit messages follow the conventional commits standard.
   - Push your branch to your fork.
   - Open a Pull Request against the `main` branch of the [SylphxAI/anymd](https://github.com/SylphxAI/anymd) repository.

## Development Setup

anymd is a Rust binary (`crates/`) published to npm through a small launcher
(`packages/`). [Bun](https://bun.sh/) runs the docs site, Biome, the
repository scripts and the TypeScript tests that drive the binary over MCP.

### Prerequisites

- Rust stable (`rustup`)
- Bun >= 1.4.0 (`packageManager` `bun@1.4.0`; install frozen from `bun.lock`)

### Getting Started

```bash
git clone https://github.com/SylphxAI/anymd.git
cd anymd
bun install
bun run build                            # cargo build --release -p anymd
node packages/anymd/bin/anymd.js version # the npm launcher finds target/release/anymd
```

### Useful Commands

```bash
bun run check          # Lint and format check (Biome)
bun run check:fix      # Auto-fix lint and format issues
bun run check:versions # Every manifest carries the same version
bun run typecheck      # TypeScript type checking (scripts/)
bun run test:rust      # Rust tests
bun run test:cov       # TypeScript tests over the built binary, with coverage
bun run docs:build     # Build docs site
```

### Coding Standards

- **Formatting and linting:** `cargo fmt` and Biome (configuration in `biome.json`). Run `bun run check` before submitting.
- **Testing:** Rust tests live beside the crates; TypeScript tests in `test/` spawn the built binary.
- **Commits:** Follow conventional commits (e.g., `feat:`, `fix:`, `docs:`, `chore:`, `refactor:`).

### Release Process

A release is a pull request that runs `bun scripts/set-version.ts X.Y.Z` and adds
a `## X.Y.Z` section to `CHANGELOG.md`. Merging it publishes every package; see
[docs/PUBLISH.md](docs/PUBLISH.md). Publishing and tags come from that workflow only.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
