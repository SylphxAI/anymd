#!/usr/bin/env bash
# Changesets version wrapper for Release workflow.
# Must be a single executable command for changesets/action `version` input.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

echo "[release-version] bump package versions from changesets"
bun node_modules/@changesets/cli/bin.js version

echo "[release-version] sync native optional package manifests to root version"
bun run native:sync-manifests

echo "[release-version] sync compatibility alias packages (@sylphx/citra, @sylphx/pdf-reader-mcp) to root version"
bun run sync:alias-packages

echo "[release-version] sync server.json + Rust SERVER_VERSION"
bun run sync:server-json

# Version bumps drift bun.lock's bundled optional-package versions/optionalDependencies.
# Synchronize every package manifest first, then refresh one lockfile for that final graph.
bun install

# Tracked dist/*.js are rebuilt earlier in Release and may gain +x from bun build.
# changesets/action commitMode=github-api rejects executable files.
if [[ -d dist ]]; then
  echo "[release-version] clear executable bits on tracked dist artifacts"
  find dist -type f \( -name '*.js' -o -name '*.mjs' \) -exec chmod a-x {} +
fi

echo "[release-version] PASS"
