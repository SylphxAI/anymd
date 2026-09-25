# Host runtime proof contract

## Rule

A platform is **runtime-proven** only when:

1. the native binary for that platform is installed, and
2. MCP `initialize` succeeds on a **matching host architecture**, and
3. the proof artifact records `runtimeProofValid=true` with host identity.

## Not accepted as host runtime proof

- Cross-compilation alone
- Installing/running an arm64 package on x64 (or vice versa)
- Rosetta-translated execution claimed as native darwin-x64
- Deferred / soft-skip legs that set `pass=true` without execute proof
- Aggregate pass while any currently required host-proven platform is missing
- **GitHub-hosted `macos-*` runners for any product Darwin build or host proof**

## Required platforms

Current status below is the audited state on 2026-08-16; artifact publication
does not upgrade a row to runtime-proven.

| Platform id | Host requirement | Current fleet status |
|---|---|---|
| `linux-x64-gnu` | Linux x86_64 | Published 5.0.0 installed launcher and exact native initialize observed on 2026-08-16; durable corrected-workflow artifact pending. |
| `linux-arm64-gnu` | Linux aarch64 | Published and cross-built only; no matching owned host pool. |
| `darwin-x64` | macOS x86_64 (not Rosetta-only labels) | Historical direct-native evidence only; no retained current installed-launcher proof. |
| `darwin-arm64` | macOS arm64 | Published build artifact only; no Apple Silicon self-hosted capacity. **Never GitHub-hosted `macos-*`.** |
| `win32-x64-msvc` | Windows x86_64 | Published and cross-built only; no matching owned host pool. |

## Self-hosted macOS policy

- Product Darwin CI must use Sylphx self-hosted labels `[self-hosted, sylphx, macos, standard]` only.
- GitHub-hosted `macos-*` runners are forbidden for product Darwin build and host proof (`PROJECT.md`).
- Current durable fleet evidence does not establish an available macOS runtime-proof host.
- `darwin-arm64` host runtime proof requires Apple Silicon self-hosted capacity. Until then, cross-build on the x64 fleet is allowed as **build evidence only** and must not be claimed as host runtime proof.

## Workflows

- Candidate build coverage: `.github/workflows/native.yml`.
  Its green state means only its declared build scope passed. Read
  `runtimeParityPass`; it is false until all five matching hosts execute the
  installed launcher.
- Published registry: `.github/workflows/registry-install-proof.yml`
- Regression guard: `bun run check:no-github-hosted-macos`
