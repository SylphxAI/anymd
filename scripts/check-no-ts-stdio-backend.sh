#!/usr/bin/env bash
# Default MCP stdio transport authority gate.
# Sole-Rust (ADR-0006): production default is pure-Rust native via dist/runtime-entry.js only.
# TypeScript PDF runtime must not be exported, shipped, or used as production fallback.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG="${ROOT}/package.json"
MATRIX="${ROOT}/docs/specs/pure-rust-capability-matrix.json"
RUST_MAIN="${ROOT}/crates/pdf-reader-mcp-server/src/main.rs"
RUNTIME_ENTRY="${ROOT}/src/runtime-entry.ts"

violations=0
report_violation() {
  echo "VIOLATION: $*"
  violations=$((violations + 1))
}

echo "=== check-no-ts-stdio-backend $(date -u +%Y-%m-%dT%H:%M:%SZ) ==="

[[ -f "${PKG}" ]] || report_violation "missing package.json"
[[ -f "${MATRIX}" ]] || report_violation "missing pure-rust capability matrix"
[[ -f "${RUST_MAIN}" ]] || report_violation "missing Rust MCP server main"

DROP_IN="$(jq -r '.productTruth.dropInFor3014' "${MATRIX}")"
SOLE="$(jq -r '.productTruth.soleRuntimeDefault // false' "${MATRIX}")"
SOLE_RUST_PROD="$(jq -r '.productTruth.soleRustProduction // false' "${MATRIX}")"
BIN_PATH="$(jq -r '.bin["anymd"]' "${PKG}")"
EXPORT_DOT="$(jq -r '.exports["."]' "${PKG}")"
EXPORT_TS="$(jq -r '.exports["./typescript"] // empty' "${PKG}")"
TS_SHIPPED="$(jq -r 'if (.productTruth | has("typescriptProductionShipped")) then .productTruth.typescriptProductionShipped | tostring else "missing" end' "${MATRIX}")"
AUTO_TS_FALLBACK="$(jq -r 'if (.productTruth | has("automaticTypescriptFallback")) then .productTruth.automaticTypescriptFallback | tostring else "missing" end' "${MATRIX}")"

if [[ "${DROP_IN}" == "true" || "${SOLE}" == "true" || "${SOLE_RUST_PROD}" == "true" ]]; then
  [[ "${BIN_PATH}" == "./dist/runtime-entry.js" ]] || report_violation "default bin must be ./dist/runtime-entry.js for sole-Rust"
  [[ "${EXPORT_DOT}" == "./dist/runtime-entry.js" ]] || report_violation "exports['.'] must be ./dist/runtime-entry.js for sole-Rust"
  [[ -f "${RUNTIME_ENTRY}" ]] || report_violation "missing src/runtime-entry.ts"
  [[ -z "${EXPORT_TS}" ]] || report_violation "exports['./typescript'] must be absent for sole-Rust production (found: ${EXPORT_TS})"
  [[ "${TS_SHIPPED}" == "false" ]] || report_violation "productTruth.typescriptProductionShipped must be false for sole-Rust (got: ${TS_SHIPPED})"
  [[ "${AUTO_TS_FALLBACK}" == "false" ]] || report_violation "productTruth.automaticTypescriptFallback must be false for sole-Rust (got: ${AUTO_TS_FALLBACK})"
  grep -q 'resolveNativeBinary\|NATIVE_PLATFORM_PACKAGES' "${RUNTIME_ENTRY}" || report_violation "runtime-entry must resolve native pure-Rust binary"
  if grep -Eq "loadTypeScriptRuntime|join\(here, ['\"]index\.js['\"]\)|// TypeScript fallback path|Falls back to the TypeScript" "${RUNTIME_ENTRY}"; then
    report_violation "runtime-entry must not load or document TypeScript production fallback"
  fi
  if grep -Eq "await import\(|dynamic.?import" "${RUNTIME_ENTRY}" && grep -q 'index.js' "${RUNTIME_ENTRY}"; then
    report_violation "runtime-entry must not dynamic-import TypeScript dist/index.js"
  fi
  grep -Eq 'TypeScript production runtime has been removed|sole-Rust|Sole-Rust' "${RUNTIME_ENTRY}" \
    || report_violation "runtime-entry must document sole-Rust / no TypeScript production runtime"
else
  # Pre-cutover historical path only (not expected on 4.0.0 sole-Rust branch).
  TS_ENTRY="${ROOT}/src/index.ts"
  [[ "${BIN_PATH}" == "./dist/index.js" ]] || report_violation "pre-cutover default bin must remain ./dist/index.js"
  [[ -f "${TS_ENTRY}" ]] || report_violation "pre-cutover TypeScript entry src/index.ts must exist"
fi

grep -q 'transport::stdio' "${RUST_MAIN}" || report_violation "Rust MCP server must expose rmcp stdio transport"

if [[ "${violations}" -gt 0 ]]; then
  echo ""
  echo "FAIL: ${violations} MCP stdio authority violation(s)."
  exit 1
fi

echo "PASS: MCP stdio authority gate (sole-Rust pure-Rust default; TypeScript production export/fallback forbidden)."
