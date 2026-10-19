#!/usr/bin/env bash

set -euo pipefail

readonly CARGO_AUDIT_VERSION="0.22.2"

if ! cargo audit --version 2>/dev/null | grep -Fq "${CARGO_AUDIT_VERSION}"; then
  cargo install cargo-audit --version "=${CARGO_AUDIT_VERSION}" --locked
fi

cargo audit --deny warnings
