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

This project uses [Bun](https://bun.sh/) and [Biome](https://biomejs.dev/).

### Prerequisites

- Bun >= 1.4.0 (`packageManager` `bun@1.4.0`; install frozen from `bun.lock`)

### Getting Started

```bash
git clone https://github.com/SylphxAI/anymd.git
cd pdf-reader-mcp
bun install
bun run build
```

### Useful Commands

```bash
bun run check          # Lint and format check (Biome)
bun run check:fix      # Auto-fix lint and format issues
bun run typecheck      # TypeScript type checking
bun test               # Run tests
bun run test:cov       # Run tests with coverage
bun run build          # Build the package
bun run package:smoke  # Verify package tarball
bun run docs:build     # Build docs site
bun run benchmark      # Run performance benchmark
```

### Coding Standards

- **Formatting and linting:** Handled by Biome (configuration in `biome.json`). Run `bun run check` before submitting.
- **Testing:** Write tests using Bun's built-in test runner. Place test files in `test/` mirroring the source structure.
- **Types:** All code must pass `bun run typecheck` with no errors.
- **Commits:** Follow conventional commits (e.g., `feat:`, `fix:`, `docs:`, `chore:`, `refactor:`).

### Release Process

Releases are automated via [Changesets](https://github.com/changesets/changesets):

1. Add a changeset describing your change: `bunx changeset`
2. The changeset bot will create a "Version Packages" PR when changesets accumulate.
3. Merging the version PR triggers the release workflow to publish to npm and create a GitHub Release.

Do not manually publish to npm or create tags/releases.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
