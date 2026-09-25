#!/usr/bin/env bash
# Download and verify the Markdown regression corpus (corpus/markdown-regression.json).
# Usage: scripts/fetch-markdown-corpus.sh [dir]   (default: .cache/markdown-corpus)
set -euo pipefail
dir="${1:-.cache/markdown-corpus}"
mkdir -p "$dir"
manifest="$(dirname "$0")/../corpus/markdown-regression.json"
jq -r '.cases[] | [.file, .url, .sha256] | @tsv' "$manifest" | while IFS=$'\t' read -r file url sha; do
  target="$dir/$file"
  if [ -f "$target" ] && echo "$sha  $target" | sha256sum --check --status; then
    continue
  fi
  curl --fail --location --retry 3 --silent --show-error -A "Mozilla/5.0 (anymd corpus)" "$url" -o "$target.tmp"
  echo "$sha  $target.tmp" | sha256sum --check --status || { echo "checksum mismatch: $url" >&2; rm -f "$target.tmp"; exit 1; }
  mv "$target.tmp" "$target"
done
echo "corpus ready in $dir"
