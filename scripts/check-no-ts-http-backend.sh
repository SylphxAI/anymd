#!/usr/bin/env bash
# Rust-First gate: Web MCP HTTP transport must not retain a parallel TS HTTP backend.
# Forbidden: HTTP bin path via node; Streamable HTTP in deleted TS MCP adapter.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${ROOT}/bin/anymd"
HTTP_TRANSPORT="${ROOT}/crates/pdf-reader-mcp-server/src/http_transport.rs"
GATE_TEST="${ROOT}/test/check-no-ts-http-backend.test.ts"
TS_ADAPTER_GATE="${ROOT}/scripts/check-ts-adapter-deletion-ready.sh"
LEDGER="${ROOT}/docs/specs/pdf-reader-mcp-migration-ledger.json"

violations=0

report_violation() {
	echo "VIOLATION: $*"
	violations=$((violations + 1))
}

echo "=== check-no-ts-http-backend $(date -u +%Y-%m-%dT%H:%M:%SZ) ==="

[[ -f "${BIN}" ]] || report_violation "missing bin/anymd"
[[ -f "${HTTP_TRANSPORT}" ]] || report_violation "missing crates/pdf-reader-mcp-server/src/http_transport.rs"
[[ -f "${GATE_TEST}" ]] || report_violation "missing test/check-no-ts-http-backend.test.ts"
[[ -f "${TS_ADAPTER_GATE}" ]] || report_violation "missing scripts/check-ts-adapter-deletion-ready.sh"
[[ -f "${LEDGER}" ]] || report_violation "missing docs/specs/pdf-reader-mcp-migration-ledger.json"

if [[ -f "${LEDGER}" ]]; then
	node - "${LEDGER}" <<'NODE'
const [ledgerPath] = process.argv.slice(2);
const ledger = JSON.parse(require("node:fs").readFileSync(ledgerPath, "utf8"));
const entry = ledger.capabilities.find((cap) => cap.id === "transport/web-mcp-http");
if (!entry) {
  console.error("[check-no-ts-http-backend] missing capability transport/web-mcp-http");
  process.exit(1);
}
if (!["rust_impl", "authority_rust", "ts_deleted"].includes(entry.state)) {
  console.error(
    `[check-no-ts-http-backend] transport/web-mcp-http is ${entry.state}; expected rust_impl, authority_rust, or ts_deleted`
  );
  process.exit(1);
}
NODE
fi

if [[ -f "${BIN}" ]]; then
	grep -q 'resolve_rust_bin' "${BIN}" || report_violation "bin/anymd must resolve Rust rmcp server via resolve_rust_bin"
	grep -q 'MCP_TRANSPORT=http' "${BIN}" || report_violation "bin/anymd must route MCP_TRANSPORT=http to Rust"
	grep -q 'transport="$(resolve_transport)"' "${BIN}" || report_violation "bin/anymd must resolve transport"
	grep -q '\[\[ "$transport" == "http" \]\]' "${BIN}" || report_violation "bin/anymd must branch on http transport"
	if grep -qE 'use_ts_transport|exec node' "${BIN}"; then
		report_violation "bin/anymd must not retain TS transport opt-in"
	fi
fi

if [[ -f "${HTTP_TRANSPORT}" ]]; then
	grep -q 'StreamableHttpService' "${HTTP_TRANSPORT}" || report_violation "Rust http_transport.rs must expose StreamableHttpService"
	grep -q 'health_check' "${HTTP_TRANSPORT}" || report_violation "Rust http_transport.rs must expose /mcp/health"
fi

if [[ "${violations}" -gt 0 ]]; then
	echo ""
	echo "FAIL: ${violations} Web MCP HTTP TS authority violation(s)."
	echo "Authority: crates/pdf-reader-mcp-server/src/http_transport.rs via bin/anymd."
	exit 1
fi

echo "PASS: Web MCP HTTP transport delegates solely to Rust rmcp (no parallel TS HTTP backend)."