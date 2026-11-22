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
   source and trust checks below. Both check the exact version on public
   `main` pushes and on manual dispatches from `main`.

## Official MCP Registry

Use the existing [Official MCP Registry entry](https://registry.modelcontextprotocol.io/v0.1/servers/io.github.SylphxAI%2Fanymd/versions/latest). Keep the exact casing of `io.github.SylphxAI/anymd`: it matches the published npm `mcpName`. Do not create a second name to fix a listing in another directory.

Before releasing, install [`mcp-publisher`](https://github.com/modelcontextprotocol/registry/releases) and run these from the repository root:

```sh
bun run check:versions
mcp-publisher validate
```

After npm publishes, check the exact version and launch the package over stdio:

```sh
version=$(node -p 'require("./server.json").version')
npm view "@sylphx/anymd@$version" mcpName bin --json
npx -y @modelcontextprotocol/inspector --cli npx -y "@sylphx/anymd@$version" mcp --method tools/list
curl -fsSL "https://registry.modelcontextprotocol.io/v0.1/servers/io.github.SylphxAI%2Fanymd/versions/$version" | jq '.server'
```

The [release workflow](../.github/workflows/release.yml) publishes from `main` only. It runs `mcp-publisher validate`, authenticates with `mcp-publisher login github-oidc`, and runs `mcp-publisher publish` only when the canonical name and version are missing. It does not update already-listed versions. To deliver a manifest correction such as the website URL or launch arguments, include it in the next version bump. After the workflow finishes, check that the Registry returns the same version, `@sylphx/anymd`, the `stdio` transport, the `mcp` package argument and `https://anymd.sylphx.com/`.

## Native CPU portability

Repository builds use `.cargo/config.toml` to disable host-specific CPU tuning
and optional x86 SIMD tiers in transcribe-cpp-sys. The conservative preset alone
does not override the SSE4.2, AVX, and F16C defaults on static macOS x86_64
builds, so those options are disabled explicitly. CI checks the compiled CMake
cache on every release target with `scripts/check-native-cpu.py`. Model-free
compilation alone does not prove CPU portability. The benchmark's AVX2 tool
build is a separate measurement, not the portable release binary's performance
guarantee.

Cargo does not inherit the repository configuration for downstream crates.io
builds. When redistributing a source build outside this checkout, set:

```sh
export TRANSCRIBE_CMAKE_ARGS="-DGGML_NATIVE=OFF -DTRANSCRIBE_X86_CONSERVATIVE=ON -DGGML_SSE42=OFF -DGGML_AVX=OFF -DGGML_F16C=OFF"
```

## winget, Homebrew and Scoop

After the shared release job succeeds, `release.yml` runs the mcp-kit
`package-managers` action in a top-level caller job on public `main` pushes.
It renders `Formula/anymd.rb` in `SylphxAI/homebrew-tap` and
`bucket/anymd.json` in `SylphxAI/scoop-bucket` from the exact version's released
archives. Each destination changes through its normal pull request and repository rules,
including a merge queue where configured; the job verifies the merged bytes
before succeeding.

The source CI identity is `anymd-package-managers`; its destination-only Broker
policy has the same name. Access exchanges the caller job's OIDC identity and
Broker issues separate destination-scoped tokens. The App signing key stays in
Broker, and no tap or bucket secrets are passed to the reusable workflow.
Missing authority fails the job instead of silently skipping either channel.
An unchanged version still reconciles both destinations, without rebuilding or
republishing existing npm packages or release assets. Repeating delivery reuses
an existing publication pull request or verifies the already-current manifest.
The pinned shared publisher also handles a new pull request becoming clean while
GitHub is enabling auto-merge: it reads the exact head and state again before one
ordinary merge request. Changed heads, blocked or closed requests, authorization
failures and a refused recovery remain failures; no administrator override is
used.

Install with `brew install sylphxai/tap/anymd`, or run
`scoop bucket add sylphx https://github.com/SylphxAI/scoop-bucket` followed by
`scoop install anymd`.

The shared workflow has no winget input, so the `winget` job in `release.yml`
does it. It hashes the published `anymd-win32-x64-msvc.zip` (which contains
`anymd.exe` at the archive root), renders the three `Sylphx.anymd` manifests
with `scripts/winget-manifest.mjs`, and opens a pull request on
`microsoft/winget-pkgs` under `manifests/s/Sylphx/anymd/<version>/`. The pull
request needs `WINGET_TOKEN`, a fine-grained personal access token that can
open pull requests on that repository, passed by name. Without the token the
job still renders and checks the manifest, then skips the pull request. A
version that already has an open or merged pull request is left alone, so a
later push to main does not open a second one. winget's own validation merges
the pull request; until it does, `winget install Sylphx.anymd` is not yet
served by the community source.

## crates.io

On a push or manual dispatch from public `main`, the `crates` job runs after the
shared release job succeeds, including when npm is already current. It reads
the exact Cargo versions of the eight crates, including the three independently
versioned forks, in dependency order from `scripts/publish-crates.sh --list`.
If all versions are already available, it does not request a credential.
Otherwise it uses crates.io OIDC to publish only absent versions and confirms
each one on crates.io. A yanked version or an unreadable registry response
fails the job; neither is treated as permission to publish. A later push or
manual dispatch from `main` resumes a partial release without republishing
versions already delivered.

`set-version.ts` moves the workspace version and the internal version pins
together. When a fork changes, bump it in `vendor/*/Cargo.toml` and in its
`[workspace.dependencies]` pin. `check:versions` compares each already-published
fork with its crates.io sources, manifest, README and license. A changed payload
must have a new version and a matching pin. CI packs every crate on each pull
request and fails a package over 9 MB (the registry limit is 10 MB).

Each crate needs a trusted-publisher record for `SylphxAI/anymd`, `release.yml`,
environment `crates-io`. If records need repair, a crate owner can check them
on crates.io or dispatch `ensure-trusted-publishers.yml` from `main` using a
setup-only `CARGO_REGISTRY_TOKEN` with the trusted-publishing scope. The setup
workflow reads back each record and reports `created`, `present` or an error.
Revoke and remove that setup credential afterward. The release workflow does
not read it. Missing publishing authority fails the release job; fixing the
records and dispatching `release.yml` on `main` retries the current version.

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
the public web app’s links updated with it.

## Recover a partial release

Re-running an old run keeps its original commit and workflow, so it cannot
pick up a release fix merged afterward. For a fix on `main`, dispatch
`release.yml` on `main` without changing the product version. npm and the
GitHub release are skipped when that version already exists; the crates job
publishes only missing versions. The image job downloads the two Linux
tarballs from the matching GitHub release and stages them into `dist/amd64/anymd` and `dist/arm64/anymd`, with no Rust
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
Ordinary pushes build the image only if that version is missing from GHCR.
A manual dispatch can rebuild it. The image job is independent of Python
delivery; it is not evidence that wheels were built or published.

The wheel job uses `scripts/recover-wheels.py` to probe the exact version on
PyPI. The helper skips
wheel building only when all five expected, non-yanked platform wheels are
listed. A version-specific 404 or an incomplete wheel list triggers recovery;
non-404 probe errors and mismatched version or wheel names fail.

Recovery downloads all five binary archives from the matching GitHub release
`vX.Y.Z`, never a latest release or a global binary. It resolves the actual tag
commit, checks its npm and Cargo versions, verifies the archives against the
existing trust job's SHA256SUMS and GitHub asset digests, and verifies each
archive's trust attestation against that tag's source commit and `release.yml`
signer. Missing targets, corrupt assets, failed attestations or a recovered
host binary reporting another version stop recovery. The wheel job waits for
the trust job to finish and requires its trust pack when building wheels.
Downloaded native files have their executable mode restored before the version
check and wheel packaging; their bytes are unchanged. Wheels pass to the PyPI
publish job through run-store.

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

For the first release of `anymd`, a PyPI account holder who will own the project
must add a pending GitHub publisher on the
[PyPI trusted publishing page](https://pypi.org/manage/account/publishing/).
If the project already exists, configure its publisher in that project's
publishing settings instead. Use these exact fields:

| PyPI field | Value |
| --- | --- |
| PyPI Project Name | `anymd` |
| Owner | `SylphxAI` |
| Repository name | `anymd` |
| Workflow name | `release.yml` |
| Environment name | `pypi` |

The project name matches the wheel metadata, not the npm scope. A valid GitHub
token followed by PyPI's `invalid-publisher` error means PyPI could not match
that job to a publisher. Check the account or project dashboard for a missing
or mismatched record. Creating the GitHub `pypi` environment alone does not
register anything on PyPI. There is no token fallback.

After confirming the record, dispatch `release.yml` from `main` without a
version bump. The job recovers the five wheels from the exact release if needed,
then submits them via OIDC. It skips existing files and verifies that PyPI lists
all five expected non-yanked wheels for the exact version before reporting
success. If the initial probe already sees all five, the wheel build and publish
are no-ops. Until this check passes in a production run, leave Python publication
marked as pending on the install pages.

## Doc-VLM builds

The default binary is built without the `ocr-vlm` feature. VLM OCR is the `anymd-ocr-vlm` companion executable (`cargo build --release -p anymd --features ocr-vlm --bin anymd-ocr-vlm`), which `release.yml` builds per platform and uploads as `anymd-ocr-vlm-<platform>` with a checksum manifest `anymd-ocr-vlm-SHA256SUMS` on the GitHub release; `anymd setup ocr` fetches it next to the weights. The `companion` job signs the manifest (`scripts/sign-companion-manifest.py`, secret `ANYMD_COMPANION_SIGNING_KEY` in the GitHub environment `companion-signing`) and uploads `anymd-ocr-vlm-SHA256SUMS.sig` before the manifest; `anymd` verifies it against `COMPANION_PUBLIC_KEYS` in `crates/anymd/src/ocr_vlm.rs`, and rotation is a new key added to that list. Immutable releases must stay off, because companions upload after the release is published. A source build with `--features ocr-vlm` runs the engine in-process and needs no companion. macOS includes Metal. `.github/workflows/docvlm.yml` checks Linux x64, Linux arm64, macOS arm64 and Windows x64, with optional measurements of float, q8 and q4 CPU weights. The release still includes its existing darwin-x64 package.

Linux arm64 needs the FP16 assembler flag for the upstream GEMM dependency. Repository builds inherit it from `.cargo/config.toml`; external `cargo install` builds need `RUSTFLAGS="-C target-feature=+fp16"`. CI extracts the published OCR-runtime package outside the repository and checks it with that explicit flag, so the package gate does not inherit `.cargo/config.toml`. The runtime checks actual kernel hardware capabilities rather than compiler-folded feature detection. Old ARM CPUs keep the plain CLI/tesseract route, checked in CI on an emulated Cortex-A72.

`CI` also accepts a manual dispatch for the same full check, including the queue-only macOS and Windows workspace tests. Model weights never ship in release packages; users explicitly install SHA-256-pinned files with `anymd setup ocr`.
