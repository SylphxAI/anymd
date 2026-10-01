#!/usr/bin/env bash
# Publish the anymd crates to crates.io in dependency order, skipping any
# version that is already there, so a rerun finishes a half-done release.
# Needs CARGO_REGISTRY_TOKEN. Pass --list to print the crates and versions only.
set -euo pipefail

# Dependency order: forks first, then the crates that use them, the binary last.
CRATES=(anymd-oar-ocr-vl anymd-adobe-cmap-parser anymd-pdf-extract anymd-pdf anymd-formats anymd-core anymd-ocr-vlm anymd)

version_of() {
  cargo metadata --no-deps --format-version 1 |
    python3 -c 'import json,sys; n=sys.argv[1]; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]==n))' "$1"
}

published() {
  local code
  code=$(curl -s -o /dev/null -w '%{http_code}' -A 'anymd-release (hi@sylphx.com)' \
    "https://crates.io/api/v1/crates/$1/$2")
  case "$code" in
    200) return 0 ;;
    404) return 1 ;;
    *) echo "crates.io answered $code for $1 $2" >&2; exit 1 ;;
  esac
}

for crate in "${CRATES[@]}"; do
  version=$(version_of "$crate")
  if [ "${1:-}" = "--list" ]; then echo "$crate $version"; continue; fi
  if published "$crate" "$version"; then
    echo "$crate $version is already on crates.io, skipping"
    continue
  fi
  echo "publishing $crate $version"
  cargo publish --locked -p "$crate"
done
