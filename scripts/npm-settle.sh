#!/usr/bin/env bash
# Post-publish checks wait for npm to serve a fresh version. `npm publish`
# returns before every registry edge knows the version, so a check run seconds
# later can see "No matching version found" (ETARGET) or 404. Source this file
# and run each check through `settle`; it retries only registry-propagation
# failures, with growing pauses, logs every attempt, and fails after the bound.
#
#   SETTLE_BOUND  total seconds to wait (default 600)
#   SETTLE_FIRST  first pause in seconds (default 5); doubles up to SETTLE_MAX
#   SETTLE_MAX    longest pause in seconds (default 30)

settle() {
  local label=$1; shift
  local bound=${SETTLE_BOUND:-600} pause=${SETTLE_FIRST:-5} cap=${SETTLE_MAX:-30}
  local start=$SECONDS attempt=0 out status
  while :; do
    attempt=$((attempt + 1))
    status=0
    out=$("$@" 2>&1) || status=$?
    if [ "$status" -eq 0 ]; then
      [ -n "$out" ] && printf '%s\n' "$out"
      echo "ok: $label (attempt $attempt, $((SECONDS - start))s)"
      return 0
    fi
    if ! printf '%s' "$out" | grep -qiE 'ETARGET|E404|No matching version|notarget|404 Not Found'; then
      printf '%s\n' "$out" >&2
      echo "FAILED: $label is not a registry delay (exit $status)" >&2
      return "$status"
    fi
    if [ $((SECONDS - start + pause)) -gt "$bound" ]; then
      printf '%s\n' "$out" >&2
      echo "FAILED: $label still not served by the npm registry after $((SECONDS - start))s ($attempt attempts)" >&2
      return 1
    fi
    echo "wait: $label not served yet (attempt $attempt, $((SECONDS - start))s); retrying in ${pause}s"
    sleep "$pause"
    pause=$((pause * 2))
    [ "$pause" -gt "$cap" ] && pause=$cap
  done
}

# Every package of this release (natives, main, aliases) must resolve at $1.
settle_all_packages() {
  local version=$1 dir name
  for dir in packages/npm/*/ packages/anymd packages/aliases/*/; do
    name=$(node -p "require('./${dir%/}/package.json').name")
    settle "$name@$version resolves" npm view "$name@$version" version
  done
}
