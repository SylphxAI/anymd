#!/usr/bin/env bash
# Build the browser playground's WebAssembly module into docs/public/wasm/.
# Needs: rustup target wasm32-unknown-unknown, wasm-bindgen CLI matching the
# pinned `wasm-bindgen` crate (crates/anymd-wasm/Cargo.toml). wasm-opt is optional.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/docs/public/wasm"
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export CARGO_HOME
export PATH="$CARGO_HOME/bin:$PATH"
bindgen_version="$(python3 - "$root/Cargo.lock" <<'PY'
import sys
import tomllib
with open(sys.argv[1], "rb") as lockfile:
    versions = [p["version"] for p in tomllib.load(lockfile)["package"] if p["name"] == "wasm-bindgen"]
assert len(versions) == 1, versions
print(versions[0])
PY
)"
if ! command -v wasm-bindgen >/dev/null 2>&1 || [ "$(wasm-bindgen --version)" != "wasm-bindgen $bindgen_version" ]; then
  cargo install --locked --force --version "=$bindgen_version" wasm-bindgen-cli
fi
hash -r
[ "$(wasm-bindgen --version)" = "wasm-bindgen $bindgen_version" ] || {
  echo "wasm-bindgen CLI does not match Cargo.lock ($bindgen_version)" >&2
  exit 1
}
cargo build --manifest-path "$root/Cargo.toml" -p anymd-wasm --profile wasm-release --target wasm32-unknown-unknown
target_dir="$(cargo metadata --manifest-path "$root/Cargo.toml" --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
rm -rf "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" \
  "$target_dir/wasm32-unknown-unknown/wasm-release/anymd_wasm.wasm"
# Optional ~8% size pass; an older wasm-opt that cannot read the module is skipped.
if command -v wasm-opt >/dev/null 2>&1; then
  if wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-mutable-globals "$out/anymd_wasm_bg.wasm" -o "$out/anymd_wasm_bg.opt.wasm"; then
    mv "$out/anymd_wasm_bg.opt.wasm" "$out/anymd_wasm_bg.wasm"
  else
    rm -f "$out/anymd_wasm_bg.opt.wasm"
    echo "wasm-opt failed; keeping the unoptimized module" >&2
  fi
fi
ls -l "$out"
