#!/usr/bin/env bash
# Reconcile the exact anymd crate versions in dependency order. --list prints
# the manifest set; --plan reports whether any version is missing, without
# requiring OIDC. Only the publish invocation uses the short-lived token.
set -euo pipefail

CRATES=(anymd-oar-ocr-vl anymd-adobe-cmap-parser anymd-pdf-extract anymd-pdf anymd-formats anymd-core anymd-ocr-vlm anymd)
mode=${1:-}
case "$mode" in
  ''|--list|--plan) ;;
  *) echo "usage: $0 [--list|--plan]" >&2; exit 2 ;;
esac

metadata=$(cargo metadata --no-deps --format-version 1)
versions=()
for crate in "${CRATES[@]}"; do
  version=$(jq -er --arg crate "$crate" '.packages[] | select(.name == $crate) | .version' <<< "$metadata")
  versions+=("$version")
  if [ "$mode" = --list ]; then echo "$crate $version"; fi
done
if [ "$mode" = --list ]; then exit 0; fi

response=$(mktemp)
trap 'rm -f -- "$response"' EXIT

# 0: exact, non-yanked version. 1: absent. 2: invalid/unknown; never publish
# (or silently treat the job as complete) if the registry cannot be checked.
published() {
  local crate=$1 version=$2 code
  code=$(curl -sS --max-time 15 --retry 2 --retry-max-time 30 -o "$response" -w '%{http_code}' \
    -A 'anymd-release (hi@sylphx.com)' -H 'Cache-Control: no-cache' \
    "https://crates.io/api/v1/crates/$crate/$version") || return 2
  case "$code" in
    200)
      if ! jq -e --arg crate "$crate" --arg version "$version" \
        '.version | .crate == $crate and .num == $version and .yanked == false' "$response" >/dev/null; then
        echo "crates.io returned an unexpected or yanked version for $crate $version" >&2
        return 2
      fi
      return 0
      ;;
    404) return 1 ;;
    *) echo "crates.io answered $code for $crate $version" >&2; return 2 ;;
  esac
}

wait_for_publication() {
  local crate=$1 version=$2 delay
  for delay in 0 5 10 20 30; do
    if [ "$delay" -gt 0 ]; then sleep "$delay"; fi
    if published "$crate" "$version"; then
      echo "verified $crate $version on crates.io"
      return 0
    elif [ "$?" -ne 1 ]; then
      return 2
    fi
  done
  echo "crates.io did not confirm publication of $crate $version" >&2
  return 1
}

missing=()
for index in "${!CRATES[@]}"; do
  crate=${CRATES[$index]}
  version=${versions[$index]}
  if published "$crate" "$version"; then
    echo "$crate $version is already on crates.io" >&2
  elif [ "$?" -eq 1 ]; then
    echo "$crate $version is missing from crates.io" >&2
    missing+=("$index")
  else
    exit 1
  fi
done

if [ "$mode" = --plan ]; then
  if [ "${#missing[@]}" -gt 0 ]; then echo needed=true; else echo needed=false; fi
elif [ "${#missing[@]}" -gt 0 ]; then
  : "${CARGO_REGISTRY_TOKEN:?a crates.io trusted-publishing token is required}"
  for index in "${missing[@]}"; do
    crate=${CRATES[$index]}
    version=${versions[$index]}
    echo "publishing $crate $version"
    cargo publish --locked --registry crates-io -p "$crate"
    wait_for_publication "$crate" "$version"
  done
fi
