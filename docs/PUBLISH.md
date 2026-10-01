# Publishing anymd

| Field | Value |
| --- | --- |
| npm package | `@sylphx/anymd` (bin `anymd`), in `packages/anymd` |
| Platform packages | `@sylphx/anymd-<platform>` for darwin-arm64, darwin-x64, linux-x64-gnu, linux-arm64-gnu, win32-x64-msvc, in `packages/npm/<platform>` |
| Alias packages | `@sylphx/citra` (bin `citra`) and `@sylphx/pdf-reader-mcp` (bin `pdf-reader-mcp`), in `packages/aliases/` |
| MCP Registry | `io.github.SylphxAI/anymd`; the old names `io.github.SylphxAI/citra` and `io.github.SylphxAI/pdf-reader-mcp` are marked deprecated |
| crates.io | `anymd` (binary, `cargo install anymd`), `anymd-core`, `anymd-formats`, `anymd-pdf`, and the forks `anymd-pdf-extract` and `anymd-adobe-cmap-parser` (from `vendor/`); `anymd-wasm` is not published |
| Release workflow | `.github/workflows/release.yml`, which calls the shared [mcp-kit release workflow](https://github.com/SylphxAI/mcp-kit) |

## How a release happens

1. In a pull request, run `bun scripts/set-version.ts X.Y.Z`, then `cargo update -w`,
   and add a `## X.Y.Z` section to `CHANGELOG.md`. The script sets the version in
   every npm manifest, `server.json` and the Cargo workspace; the binary reports
   the Cargo version, so `anymd version` prints `anymd X.Y.Z`. CI fails when the
   manifests disagree (`bun run check:versions`).
2. Merging to `main` runs `release.yml`. When `@sylphx/anymd@X.Y.Z` is not on
   npm yet, the mcp-kit workflow:
   - builds the binary for the 5 platforms and runs `anymd version` where it can,
   - publishes the platform packages, then `@sylphx/anymd`, then the two aliases,
   - runs the smoke test with `npx`: `version`, a conversion of
     `test/fixtures/sample.pdf`, and `version` through both aliases,
   - creates the GitHub release `vX.Y.Z` with the binaries and the `CHANGELOG.md`
     section as notes,
   - publishes `server.json` to the MCP Registry and marks the old names deprecated.

   A push whose version is already on npm does nothing, so every other merge is a
   no-op for publishing. A failed run can be re-run; each step skips what is
   already published.

## Native CPU portability

Repository builds use `.cargo/config.toml` to pass `GGML_NATIVE=OFF` and
`TRANSCRIBE_X86_CONSERVATIVE=ON` to transcribe-cpp-sys. This disables host-specific
CPU tuning and optional x86 SIMD tiers: a binary built on a recent CI CPU can
run on an older supported CPU. CI checks the compiled CMake cache on every
release target with `scripts/check-native-cpu.py`. Model-free compilation alone
does not prove CPU portability. The benchmark's AVX2 tool build is a separate
measurement, not the portable release binary's performance guarantee.

When redistributing a source build outside this checkout, set
`TRANSCRIBE_CMAKE_ARGS="-DGGML_NATIVE=OFF -DTRANSCRIBE_X86_CONSERVATIVE=ON"`
before building; Cargo's repository configuration is not inherited by downstream
crates.io consumers.

## crates.io

The `crates` job in `release.yml` runs after the release job succeeds and calls
`scripts/publish-crates.sh`, which publishes the six crates in dependency order
(the two forks, `anymd-pdf`, `anymd-formats`, `anymd-core`, `anymd`) with the
organization secret `CARGO_REGISTRY_TOKEN`, and skips any version already on
crates.io. `set-version.ts` moves the workspace version and the internal
`version` pins together; the forks have their own versions
(`anymd-pdf-extract` 0.12.2, `anymd-adobe-cmap-parser` 0.4.1), and their
version is raised by hand in `vendor/*/Cargo.toml` (and in the `[workspace.dependencies]`
pin) when the fork changes. `check:versions` compares each already-published fork
with its crates.io package (sources, manifest, README and license), without
compiling. A changed payload must have a new version and a matching workspace
pin; registry errors fail the check. CI packs every crate on each pull request
(`cargo package` for each crate) and fails a package over 9 MB (the limit is 10 MB).
The token needs the scopes `publish-new` and `publish-update`.

## npm trusted publishing

Publishing uses npm trusted publishing (GitHub OIDC); no npm token is stored.
npm checks the calling workflow file, so all 8 packages (`@sylphx/anymd`, the 5
platform packages, `@sylphx/citra`, `@sylphx/pdf-reader-mcp`) trust GitHub
Actions, organization `SylphxAI`, repository `anymd`, workflow `release.yml`, no
environment. To set it for one package:

```bash
npm trust github @sylphx/anymd --file release.yml --repo SylphxAI/anymd --allow-publish --otp <code>
```

Each package's `repository.url` must stay `git+https://github.com/SylphxAI/anymd.git`;
npm compares it with the publishing repository.

## Aliases

The alias packages depend on `@sylphx/anymd` at the same version and run its
launcher, so `citra` and `pdf-reader-mcp` behave exactly like `anymd`.

## Repository slug

Project site URLs do not redirect on rename. Renaming the repository moves the
docs site path behind `websiteUrl` and `homepage`, and needs `base` in
`docs/.vitepress/config.ts` updated with it.

## Recover a partial release

Re-running an old run keeps its original commit and workflow, so it cannot
pick up a release fix merged afterward. For a fix on `main`, dispatch
`release.yml` on `main` without changing the product version. npm and the
GitHub release are skipped when that version already exists; the crates job
publishes only missing versions. On a dispatch with no native artifacts, the
image job downloads the two Linux tarballs from the matching GitHub release
and stages them into `dist/amd64/anymd` and `dist/arm64/anymd`, with no Rust
compile. `Dockerfile.release.dockerignore` includes those binaries and the root
`LICENSE` in the build context. The image ships that file at
`/usr/share/licenses/anymd/LICENSE`, including any bundled third-party notices,
and the release job compares its contents with the checkout after publishing.
The OCI metadata includes `org.opencontainers.image.licenses=MIT`.
Ordinary pushes without new binaries skip the image job.

## Python wheel payload

`scripts/build-wheels.py` packages the same release binary as a scripts entry
and includes `packages/pypi/anymd` as an importable Python package in that wheel.
The wrapper never downloads another binary or implements conversion. Base
requirements stay empty; `langchain` and `llamaindex` extras declare their
optional core framework dependencies. All payload files are hashed in `RECORD`.
No separate Python version or release workflow is introduced.

CI runs the standard-library API/packaging tests, then installs a wheel built
from its native binary with both extras and tests the actual adapters and small
PDF/CSV fixtures. The release smoke checks both the installed CLI and Python
API before `twine check`. Locally, use
`python3 -m unittest discover -s packages/pypi/tests -v`; set `ANYMD_BIN` to an
existing native binary for fixture tests and install the extras to run framework
tests. Neither these checks nor the examples download models.

PyPI uses the trusted publisher for owner `SylphxAI`, repository `anymd`,
workflow `release.yml`, environment `pypi`. That publisher must be registered
on PyPI before OIDC token exchange can succeed; an image/crates recovery
dispatch has no new wheels and does not retry PyPI.
