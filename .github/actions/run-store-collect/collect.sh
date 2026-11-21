#!/usr/bin/env bash
set -euo pipefail

client="$GITHUB_WORKSPACE/.run-store-tools/.github/actions/run-store/run-store.sh"
mkdir -p "$DEST"
receipt=$(mktemp)
trap 'rm -f -- "$receipt"' EXIT
while IFS= read -r name; do
  [[ -n "$name" ]] || continue
  [[ "$name" =~ ^[A-Za-z0-9._-]{1,100}$ && "$name" != . && "$name" != .. ]] || exit 1
  target="$DEST"
  if [[ "$MERGE" != true ]]; then target="$DEST/$name"; fi
  : > "$receipt"
  NAME="$name" STORE_PATH="$target" GITHUB_OUTPUT="$receipt" bash "$client"
  if grep -qx 'unavailable=true' "$receipt"; then
    echo "::error::run-store was unavailable while fetching $name" >&2
    exit 1
  fi
done <<< "$NAMES"
