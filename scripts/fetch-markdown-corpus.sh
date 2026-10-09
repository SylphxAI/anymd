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
  # curl's default retry set excludes TLS handshake errors (exit 35). These
  # immutable GETs are safe to retry; keep both attempts and total time bounded.
  # -o lets curl reset partial output before retrying, unlike shell redirection.
  curl --fail --location --retry 3 --retry-all-errors --retry-max-time 120 \
    --connect-timeout 15 --max-time 60 --silent --show-error \
    -A "Mozilla/5.0 (anymd corpus)" "$url" -o "$target.tmp"
  echo "$sha  $target.tmp" | sha256sum --check --status || { echo "checksum mismatch: $url" >&2; rm -f "$target.tmp"; exit 1; }
  mv "$target.tmp" "$target"
done
echo "corpus ready in $dir"
