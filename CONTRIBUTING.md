# Contributing to PDF Reader MCP

Thank you for considering contributing! We welcome contributions from the community.

## How to Contribute

1. **Reporting Issues:** If you find a bug or have a feature request, please open an issue on GitHub.
   - Provide a clear description of the issue.
   - Include steps to reproduce (for bugs).
   - Explain the motivation for the feature request.

2. **Submitting Pull Requests:**
   - Fork the repository.
   - Create a new branch for your feature or bugfix (e.g., `feature/new-pdf-feature` or `bugfix/parsing-error`).
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
[docs/PUBLISH.md](docs/PUBLISH.md). Do not publish to npm or create tags by hand.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
