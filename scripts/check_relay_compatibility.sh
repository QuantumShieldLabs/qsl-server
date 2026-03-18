#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-}"
ENV_FILE="${ENV_FILE:-/etc/qsl-server/relay.env}"
CHANNEL="${CHANNEL:-compat-probe}"
TIMEOUT="${TIMEOUT:-8}"

if [[ -z "$BASE_URL" ]]; then
  echo "QSL_RELAY_COMPAT_RESULT FAIL code=missing_base_url"
  exit 1
fi

base="${BASE_URL%/}"
auth_args=()
if [[ -f "$ENV_FILE" ]]; then
  relay_token="$(awk -F= '$1=="RELAY_TOKEN"{print $2}' "$ENV_FILE" | tail -n 1 | tr -d '[:space:]' || true)"
  if [[ -n "$relay_token" ]]; then
    auth_args=(-H "Authorization: Bearer ${relay_token}")
  fi
fi

canonical_url="${base}/v1/pull?max=1"
legacy_url="${base}/v1/pull/${CHANNEL}?max=1"
canonical_status="$(
  curl -sS -o /dev/null -w '%{http_code}' --max-time "$TIMEOUT" \
    "${auth_args[@]}" \
    -H "X-QSL-Route-Token: ${CHANNEL}" \
    "$canonical_url" || true
)"
legacy_status="$(
  curl -sS -o /dev/null -w '%{http_code}' --max-time "$TIMEOUT" \
    "${auth_args[@]}" \
    -H "X-QSL-Route-Token: ${CHANNEL}" \
    "$legacy_url" || true
)"

canonical_status="${canonical_status:-000}"
legacy_status="${legacy_status:-000}"

echo "QSL_RELAY_COMPAT base=${base} canonical_status=${canonical_status} legacy_status=${legacy_status}"

case "$canonical_status" in
  200|204|401) ;;
  404|405)
    if [[ "$legacy_status" == "200" || "$legacy_status" == "204" || "$legacy_status" == "401" ]]; then
      echo "QSL_RELAY_COMPAT_RESULT FAIL code=legacy_only_deploy"
      exit 1
    fi
    echo "QSL_RELAY_COMPAT_RESULT FAIL code=canonical_unavailable"
    exit 1
    ;;
  *)
    echo "QSL_RELAY_COMPAT_RESULT FAIL code=canonical_status_${canonical_status}"
    exit 1
    ;;
esac

if [[ "$legacy_status" == "200" || "$legacy_status" == "204" || "$legacy_status" == "401" ]]; then
  echo "QSL_RELAY_COMPAT_RESULT PASS code=canonical_ok legacy_compat=present"
else
  echo "QSL_RELAY_COMPAT_RESULT PASS code=canonical_ok legacy_compat=absent_or_disabled"
fi
