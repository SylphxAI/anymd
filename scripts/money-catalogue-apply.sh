#!/usr/bin/env bash
# Apply config/money-catalogue.json through this repository's pinned OIDC workflow.
set -euo pipefail

for tool in curl jq awk sed sort; do
  command -v "$tool" >/dev/null 2>&1 || { echo "::error::$tool is needed on the runner"; exit 1; }
done
missing=""
for name in ACCESS_NAME ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_ID_TOKEN_REQUEST_TOKEN; do
  if [ -z "${!name:-}" ]; then missing="$missing $name"; fi
done
if [ -n "$missing" ]; then
  echo "::error::missing:$missing. The OIDC variables need permissions: id-token: write."
  exit 1
fi
work="${RUNNER_TEMP:-/tmp}"

jwt=$(curl -sS --fail-with-body \
  -H "Authorization: Bearer ${ACTIONS_ID_TOKEN_REQUEST_TOKEN}" \
  "${ACTIONS_ID_TOKEN_REQUEST_URL}&audience=sylphx-access" | jq -r '.value')
if [ -z "$jwt" ] || [ "$jwt" = "null" ]; then
  echo "::error::GitHub did not return an OIDC token"; exit 1
fi
echo "::add-mask::${jwt}"
body=$(jq -cn --arg name "$ACCESS_NAME" --arg token "$jwt" '{name:$name,token:$token}')
code=$(curl -sS -o "$work/access.out" -w '%{http_code}' -X POST \
  "https://api.sylphx.com/v1/access/github/token" \
  -H 'Content-Type: application/json' --data "$body")
if [ "$code" -ge 300 ]; then
  echo "::error::Access refused the token exchange for ${ACCESS_NAME} (${code})"; exit 1
fi
key=$(jq -r '.api_key//empty' "$work/access.out")
if [ -z "$key" ]; then
  echo "::error::Access answered without an api_key"; exit 1
fi
echo "::add-mask::${key}"
resource=$(jq -r '.key.name//empty' "$work/access.out" | sed 's#/api_keys/.*$##')
active_after_ms=$(jq -r '.active_after_ms//0' "$work/access.out")
rm -f "$work/access.out"
case "$resource" in
  orgs/*/projects/*/envs/*) ;;
  *) echo "::error::the key is not scoped to an environment"; exit 1 ;;
esac
if [ "$active_after_ms" -gt 0 ]; then
  sleep "$(awk -v ms="$active_after_ms" 'BEGIN { printf "%.3f", ms/1000 }')"
fi

call_with_retry() {
  local method="$1" url="$2" out="$3"; shift 3
  local attempt=1 code
  while :; do
    code=$(curl -sS -o "$out" -w '%{http_code}' -X "$method" "$url" \
      -H "Authorization: Bearer ${key}" -H 'Content-Type: application/json' "$@")
    if [ "$code" = "401" ] && grep -q 'not_yet_propagated' "$out" && [ "$attempt" -lt 8 ]; then
      attempt=$((attempt+1))
      echo "::warning::$method ${url##*/} answered 401 not_yet_propagated; retry ${attempt}/8 after 15 s" >&2
      sleep 15
      continue
    fi
    printf '%s' "$code"
    return 0
  done
}

require_test_merchant() {
  if ! jq -e '.livemode == false and .state == "connected" and .charges_enabled == true' "$1" >/dev/null; then
    echo "::error::merchant account is not a connected, charges-enabled Stripe test account"; exit 1
  fi
}
ensure_merchant_connection() {
  if [ -z "${STRIPE_TEST_RESTRICTED_KEY:-}" ]; then return 0; fi
  case "$STRIPE_TEST_RESTRICTED_KEY" in
    rk_test_*) ;;
    *) echo "::error::STRIPE_TEST_RESTRICTED_KEY must start with rk_test_"; exit 1 ;;
  esac
  local url code
  url="https://api.sylphx.com/v1/${resource}/merchant_accounts/default"
  code=$(call_with_retry GET "$url" "$work/merchant-get.out")
  if [ "$code" = 200 ]; then
    require_test_merchant "$work/merchant-get.out"
    return 0
  fi
  if [ "$code" != 404 ]; then
    echo "::error::GET merchant account answered ${code}"; cat "$work/merchant-get.out"; exit 1
  fi
  code=$(call_with_retry POST "https://api.sylphx.com/v1/${resource}/merchant_accounts:connect" \
    "$work/merchant-connect.out" --data "$(jq -cn --arg key "$STRIPE_TEST_RESTRICTED_KEY" '{restricted_key:$key}')")
  if [ "$code" -ge 300 ]; then
    # Never print a processor response on this secret-bearing route.
    echo "::error::POST merchant_accounts:connect answered ${code}"; exit 1
  fi
  code=$(call_with_retry GET "$url" "$work/merchant-readback.out")
  if [ "$code" != 200 ]; then
    echo "::error::merchant account readback answered ${code}"; cat "$work/merchant-readback.out"; exit 1
  fi
  require_test_merchant "$work/merchant-readback.out"
}
ensure_merchant_connection

ensure_licence_keys() {
  local catalogue="$1" key_id url code
  while IFS= read -r key_id; do
    if [[ ! "$key_id" =~ ^[a-z][a-z0-9_-]{0,62}$ ]] || [[ "$key_id" == *- ]] || [[ "$key_id" == *_ ]]; then
      echo "::error::invalid licence key id in catalogue"; exit 1
    fi
    url="https://api.sylphx.com/v1/${resource}/licence_keys/${key_id}"
    code=$(call_with_retry GET "$url" "$work/licence-get.out")
    if [ "$code" = 200 ]; then continue; fi
    if [ "$code" != 404 ]; then
      echo "::error::GET licence key ${key_id} answered ${code}"; cat "$work/licence-get.out"; exit 1
    fi
    code=$(call_with_retry POST "https://api.sylphx.com/v1/${resource}/licence_keys:generate" \
      "$work/licence-generate.out" --data "$(jq -cn --arg id "$key_id" '{licence_key_id:$id}')")
    if [ "$code" -ge 300 ] && [ "$code" != 409 ]; then
      echo "::error::POST licence_keys:generate for ${key_id} answered ${code}"; cat "$work/licence-generate.out"; exit 1
    fi
    code=$(call_with_retry GET "$url" "$work/licence-readback.out")
    if [ "$code" != 200 ]; then
      echo "::error::licence key ${key_id} readback answered ${code}"; cat "$work/licence-readback.out"; exit 1
    fi
  done < <(jq -r '.spec.products[]?.licence?.key//empty' "$catalogue" | sort -u)
}
ensure_licence_keys config/money-catalogue.json

base="https://api.sylphx.com/v1/${resource}/price_catalogs/default"
code=$(call_with_retry PATCH "${base}?allow_missing=true" "$work/patch.out" --data @config/money-catalogue.json)
if [ "$code" -ge 300 ]; then
  echo "::error::PATCH price_catalogs/default answered ${code}"; cat "$work/patch.out"; exit 1
fi
code=$(call_with_retry POST "${base}:sync" "$work/sync.out" --data '{}')
if [ "$code" -ge 300 ]; then
  if [ "${SYNC_FAILURE_NONFATAL:-}" = "1" ] && [ -z "${STRIPE_TEST_RESTRICTED_KEY:-}" ] \
    && [ "$code" = "400" ] \
    && jq -e '.code == "INVALID_STATE" and ([.details[]?.reason] == ["merchant_not_connected"])' "$work/sync.out" >/dev/null; then
    echo "::warning::preview has no Stripe test-mode connection; its sole merchant_not_connected response is not blocking production"
    echo "catalogue patched on ${resource##*/}, not synced"
    exit 0
  fi
  echo "::error::POST price_catalogs/default:sync answered ${code}"; cat "$work/sync.out"; exit 1
fi
echo "catalogue applied to ${resource##*/}"
