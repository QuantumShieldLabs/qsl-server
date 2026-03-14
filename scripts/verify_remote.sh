#!/usr/bin/env bash
set -euo pipefail

TS="$(date -u +%Y%m%dT%H%M%SZ)"
LOG="/var/log/qsl-server/verify_${TS}.log"
BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
CH="${CHANNEL:-verify-${TS}}"
BASE_DIR="${BASE_DIR:-/opt/qsl-server}"
BIN_PATH="${BASE_DIR}/bin/qsl-server"
DEPLOY_INFO="${BASE_DIR}/DEPLOYMENT_INFO"

{
  echo "TS=$TS"
  echo "BASE_URL=$BASE_URL"
  echo "CHANNEL=$CH"
  echo "--- systemd status ---"
  systemctl is-active qsl-server
  systemctl status qsl-server --no-pager
  echo "--- listener ---"
  ss -ltnp | grep -E ':8080\b' || true
  echo "--- service fragment ---"
  systemctl show -p FragmentPath --value qsl-server || true
  echo "--- deployment metadata ---"
  if [[ -f "$DEPLOY_INFO" ]]; then
    sed -n '1,40p' "$DEPLOY_INFO"
  else
    echo "DEPLOYMENT_INFO missing"
  fi
  echo "--- installed binary sha256 ---"
  if [[ -f "$BIN_PATH" ]]; then
    sha256sum "$BIN_PATH"
  else
    echo "binary missing: $BIN_PATH"
  fi
  echo "--- push/pull sanity ---"
  curl -sS -D- -o /dev/null -H "X-QSL-Route-Token: $CH" "$BASE_URL/v1/pull?max=1" | sed -n '1,25p'
  printf hello | curl -sS -D- -o /dev/null -X POST -H "X-QSL-Route-Token: $CH" "$BASE_URL/v1/push" --data-binary @- | sed -n '1,25p'
  curl -sS -D- -X GET -H "X-QSL-Route-Token: $CH" "$BASE_URL/v1/pull?max=1" | sed -n '1,80p'
} | tee "$LOG"

echo "WROTE $LOG"
