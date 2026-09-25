#!/usr/bin/env bash

set -euo pipefail

ACTIONLINT_VERSION="1.7.12"

# shellcheck integration is off: GitHub-hosted runners ship shellcheck, and it
# takes minutes-to-forever on the multi-thousand-line differential workflow.
# Workflow structure, expressions, and action inputs are still fully linted.
ACTIONLINT_FLAGS=(-config-file .github/actionlint.yaml -shellcheck=)

if command -v actionlint >/dev/null 2>&1; then
  exec actionlint "${ACTIONLINT_FLAGS[@]}" "$@"
fi

platform="$(uname -s)"
architecture="$(uname -m)"

case "${platform}/${architecture}" in
  Linux/x86_64)
    archive_platform="linux_amd64"
    archive_sha256="8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8"
    ;;
  Linux/aarch64 | Linux/arm64)
    archive_platform="linux_arm64"
    archive_sha256="325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6"
    ;;
  *)
    echo "Unsupported actionlint host: ${platform}/${architecture}" >&2
    exit 1
    ;;
esac

actionlint_tmp="$(mktemp -d)"
trap 'rm -rf -- "$actionlint_tmp"' EXIT

archive="actionlint_${ACTIONLINT_VERSION}_${archive_platform}.tar.gz"
url="https://github.com/rhysd/actionlint/releases/download/v${ACTIONLINT_VERSION}/${archive}"

curl --fail --location --retry 3 --silent --show-error "$url" --output "$actionlint_tmp/$archive"
printf '%s  %s\n' "$archive_sha256" "$actionlint_tmp/$archive" | sha256sum --check --status
tar -xzf "$actionlint_tmp/$archive" -C "$actionlint_tmp" actionlint

exec "$actionlint_tmp/actionlint" "${ACTIONLINT_FLAGS[@]}" "$@"
