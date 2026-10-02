# Publishing anymd

| Field | Value |
| --- | --- |
| npm package | `@sylphx/anymd` (bin `anymd`), in `packages/anymd` |
| Platform packages | `@sylphx/anymd-<platform>` for darwin-arm64, darwin-x64, linux-x64-gnu, linux-arm64-gnu, win32-x64-msvc, in `packages/npm/<platform>` |
| Alias packages | `@sylphx/citra` (bin `citra`) and `@sylphx/pdf-reader-mcp` (bin `pdf-reader-mcp`), in `packages/aliases/` |
| MCP Registry | `io.github.SylphxAI/anymd`; the old names `io.github.SylphxAI/citra` and `io.github.SylphxAI/pdf-reader-mcp` are marked deprecated |
| crates.io | `anymd` (binary, `cargo install anymd`), `anymd-core`, `anymd-formats`, `anymd-pdf`, `anymd-ocr-vlm`, and the forks `anymd-pdf-extract`, `anymd-adobe-cmap-parser`, and `anymd-oar-ocr-vl` (from `vendor/`); `anymd-wasm` is not published |
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

   The shared npm release skips targets already published for that version.
   That does not prove crates.io or PyPI delivery is complete: their jobs can
   still recover missing delivery without a version bump, subject to the
   source and trust checks below.

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
`scripts/publish-crates.sh`, which publishes the eight crates in dependency order
(the three forks, `anymd-pdf`, `anymd-formats`, `anymd-core`, `anymd-ocr-vlm`, `anymd`) with the
organization secret `CARGO_REGISTRY_TOKEN`, and skips any version already on
crates.io. `set-version.ts` moves the workspace version and the internal
`version` pins together; the forks have their own versions
(`anymd-pdf-extract` 0.12.2, `anymd-adobe-cmap-parser` 0.4.1, `anymd-oar-ocr-vl` 0.9.2), and their
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
The image creates the licence directory with mode `0755` before copying the
`0644` licence, so the default non-root user can traverse and read it.
The OCI metadata includes `org.opencontainers.image.licenses=MIT`.
Recovery runs the corrected Dockerfile and wheel tooling from the reviewed
workflow checkout, without replacing tagged native bytes or Python payloads.
Image staging validates both Linux identity sidecars against their bytes and
requires one original source. Its licence comes from that source commit.
`io.sylphx.native.source` records that original binary source;
`io.sylphx.packaging.source` and the image build attestation record the workflow
packaging source. These can differ during recovery; neither is relabelled as
the other.
Ordinary pushes without new binaries skip the image job. This image recovery
path is independent of Python delivery; it is not evidence that wheels were
built or published.

With no matching native artifacts, the wheel job uses
`scripts/recover-wheels.py` to probe the exact version on PyPI. The helper skips
wheel building only when all five expected, non-yanked platform wheels are
listed. A version-specific 404 or an incomplete wheel list triggers recovery;
non-404 probe errors and mismatched version or wheel names fail. A partial
native artifact matrix or an empty native binary fails instead of falling
back to release assets.

Recovery downloads all five binary archives from the matching GitHub release
`vX.Y.Z`, never a latest release or a global binary. It resolves the actual tag
commit, checks its npm and Cargo versions, verifies the archives against the
existing trust job's SHA256SUMS and GitHub asset digests, and verifies each
archive's trust attestation against that tag's source commit and `release.yml`
signer. Missing targets, corrupt assets, failed attestations or a recovered
host binary reporting another version stop recovery. The wheel job waits for
the trust job to finish; fresh native-artifact delivery does not require the
trust job to succeed. Downloaded native files have their executable mode restored
before the version check and wheel packaging; their bytes are unchanged.

The existing wheel builder packages the Python API, README and licence fetched
from that exact tag commit alongside those binaries. It must not substitute
newer API or licence files from `main`. The `v8.2.0` tag lacks the Python API,
so its recovery fails with an explicit requirement for a new release containing
the API. The 8.3 release must include that API source in its own tag before
this recovery path can be used; no historical tag is retrofitted. Offline
fixtures cover these decisions and source identities, but do not establish
live PyPI delivery.

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
on PyPI before OIDC token exchange can succeed. After registration, a fresh
recovery run can build missing wheels under the exact-tag checks above and
pass them through the existing CLI/API smoke tests and OIDC `pypi` job. The
publisher skips files already present; neither publisher registration nor a
successful image/crates job proves that the five Python wheels were delivered.

## Doc-VLM builds

The default binary is built without the `ocr-vlm` feature. VLM OCR is the `anymd-ocr-vlm` companion executable (`cargo build --release -p anymd --features ocr-vlm --bin anymd-ocr-vlm`), which `release.yml` builds per platform and uploads as `anymd-ocr-vlm-<platform>` with a checksum manifest `anymd-ocr-vlm-SHA256SUMS` on the GitHub release; `anymd setup ocr` fetches it next to the weights. The `companion` job signs the manifest (`scripts/sign-companion-manifest.py`, secret `ANYMD_COMPANION_SIGNING_KEY` in the GitHub environment `companion-signing`) and uploads `anymd-ocr-vlm-SHA256SUMS.sig` before the manifest; `anymd` verifies it against `COMPANION_PUBLIC_KEYS` in `crates/anymd/src/ocr_vlm.rs`, and rotation is a new key added to that list. Immutable releases must stay off, because companions upload after the release is published. A source build with `--features ocr-vlm` runs the engine in-process and needs no companion. macOS includes Metal. `.github/workflows/docvlm.yml` checks Linux x64, Linux arm64, macOS arm64 and Windows x64, with optional measurements of float, q8 and q4 CPU weights. The release still includes its existing darwin-x64 package.

Linux arm64 needs the FP16 assembler flag for the upstream GEMM dependency. Repository builds inherit it from `.cargo/config.toml`; external `cargo install` builds need `RUSTFLAGS="-C target-feature=+fp16"`. CI extracts the published OCR-runtime package outside the repository and checks it with that explicit flag, so the package gate does not inherit `.cargo/config.toml`. The runtime checks actual kernel hardware capabilities rather than compiler-folded feature detection. Old ARM CPUs keep the plain CLI/tesseract route, checked in CI on an emulated Cortex-A72.

`CI` also accepts a manual dispatch for the same full check, including the queue-only macOS and Windows workspace tests. Model weights never ship in release packages; users explicitly install SHA-256-pinned files with `anymd setup ocr`.
