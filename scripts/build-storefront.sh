#!/usr/bin/env bash
set -euo pipefail
export CARGO_NET_GIT_FETCH_WITH_CLI=true

# Build the product-owned Keel checkout/recovery pages beside the existing
# VitePress documentation. The central checkout is not used by these pages.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

KEEL_REV="${KEEL_REV:-b993eac24f2042698dc814b13e4f4a8457c2b32a}"
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
CARGO_BIN="${CARGO_BIN:-cargo}"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${TMPDIR:-/tmp}/anymd-keel-target-${KEEL_REV:0:12}}"
CARGO_BUILD_BUILD_DIR="${CARGO_BUILD_BUILD_DIR:-$CARGO_TARGET_DIR}"
export CARGO_TARGET_DIR CARGO_BUILD_BUILD_DIR
DIST="${STOREFRONT_DIST:-docs/.vitepress/dist}"
PREBUILT="docs/storefront/prebuilt"
OUT="$(mktemp -d "${TMPDIR:-/tmp}/anymd-storefront.XXXXXX")"
trap 'rm -rf "$OUT"' EXIT

# The CLI crate owns the canonical Money config; Keel consumes the same bytes.
cp crates/anymd/src/money_docs.toml docs/storefront/docs.toml

# Public pull-request runners must not receive private Keel source or a source
# credential. They verify and compose the committed public engine output. A
# maintainer's local build regenerates that output from the exact pinned Keel.
USE_PREBUILT="${STOREFRONT_USE_PREBUILT:-}"
if [ -z "$USE_PREBUILT" ]; then
  if [ "${GITHUB_ACTIONS:-}" = "true" ]; then USE_PREBUILT=1; else USE_PREBUILT=0; fi
fi
if [ "$USE_PREBUILT" != "1" ]; then
  if [ "${STOREFRONT_SKIP_KEEL_INSTALL:-0}" != "1" ]; then
    "$CARGO_BIN" install --locked --git https://github.com/SylphxAI/keel --rev "$KEEL_REV" sylphx-keel-cli --bin keel
  fi
  export PATH="$CARGO_HOME/bin:$PATH"
  keel docs build docs/storefront --out "$OUT/out"
  rm -rf "$PREBUILT"
  mkdir -p "$PREBUILT"
  cp "$OUT/out/pricing.html" "$PREBUILT/pricing.html"
  cp "$OUT/out/recover.html" "$PREBUILT/recover.html"
  cp -R "$OUT/out/_docs" "$PREBUILT/"
  cp -R "$OUT/out/_keel" "$PREBUILT/"
  python3 scripts/storefront_artifact.py write "$ROOT" "$KEEL_REV"
fi
python3 scripts/storefront_artifact.py verify "$ROOT" "$KEEL_REV"

# VitePress owns the rest of dist. Copy only the Keel pages and their runtime,
# never its root index or other generated site metadata.
mkdir -p "$DIST"
cp "$PREBUILT/pricing.html" "$DIST/pricing.html"
cp "$PREBUILT/recover.html" "$DIST/recover.html"
rm -rf "$DIST/_docs" "$DIST/_keel"
cp -R "$PREBUILT/_docs" "$DIST/"
cp -R "$PREBUILT/_keel" "$DIST/"

test -s "$DIST/pricing.html"
test -s "$DIST/recover.html"
grep -q 'data-keel-money-checkout' "$DIST/pricing.html"
grep -q 'data-keel-money-form="checkout"' "$DIST/pricing.html"
grep -q 'data-keel-money-form="recover"' "$DIST/recover.html"
grep -q 'src="/anymd/_keel/' "$DIST/pricing.html"
grep -q 'href="/anymd/_docs/' "$DIST/pricing.html"
! grep -q 'ProBuy\|ProTracking' "$DIST/pricing.html"
! grep -q 'buy.sylphx.com' "$DIST/pricing.html"
! grep -q 'buy.sylphx.com' "$DIST/recover.html"

printf 'Keel storefront pages copied into %s (prebuilt=%s)\n' "$DIST" "$USE_PREBUILT"
