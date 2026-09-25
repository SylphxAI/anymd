#!/usr/bin/env bash
# Fetch the benchmark corpus: the SHA-256-verified PDFs from
# corpus/markdown-regression.json plus microsoft/markitdown's MIT-licensed
# test files at a pinned commit.
set -euo pipefail
dir="${1:-.cache/bench-corpus}"
here="$(cd "$(dirname "$0")" && pwd)"
bash "$here/../scripts/fetch-markdown-corpus.sh" "$dir"
commit=b8f79c57ebc0044be41323d89b2a45d3fda8460e
base="https://raw.githubusercontent.com/microsoft/markitdown/$commit/packages/markitdown/tests/test_files"
for file in SPARSE-2024-INV-1234_borderless_table.pdf test.docx test.pptx test.xlsx test.epub test_wikipedia.html; do
  [ -s "$dir/$file" ] || curl --fail --location --retry 3 --silent --show-error "$base/$file" -o "$dir/$file"
done
echo "benchmark corpus ready in $dir"
