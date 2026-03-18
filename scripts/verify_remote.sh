#!/usr/bin/env bash
set -euo pipefail

TS="$(date -u +%Y%m%dT%H%M%SZ)"
LOG_DIR="/var/log/qsl-server"
LOG="${LOG_DIR}/verify_${TS}.log"
BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
CH="${CHANNEL:-verify-${TS}}"
BASE_DIR="${BASE_DIR:-/opt/qsl-server}"
BIN_PATH="${BASE_DIR}/bin/qsl-server"
DEPLOY_INFO="${BASE_DIR}/DEPLOYMENT_INFO"
ENV_FILE="${ENV_FILE:-/etc/qsl-server/relay.env}"
CADDY_FILE="${CADDY_FILE:-/etc/caddy/Caddyfile}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PUBLIC_BASE_URL="${PUBLIC_BASE_URL:-}"

install -d -m 0755 "$LOG_DIR"

if [[ -z "$PUBLIC_BASE_URL" && -f "$CADDY_FILE" ]]; then
  caddy_host="$(awk '
    /^[[:space:]]*#/ {next}
    /^[[:space:]]*$/ {next}
    /\{/ {
      token=$1
      gsub(/[,{]/, "", token)
      if (token != "" && token !~ /^@/) { print token; exit }
    }
  ' "$CADDY_FILE" || true)"
  if [[ -n "${caddy_host:-}" ]]; then
    PUBLIC_BASE_URL="https://${caddy_host}"
  fi
fi

{
  echo "TS=$TS"
  echo "BASE_URL=$BASE_URL"
  echo "PUBLIC_BASE_URL=${PUBLIC_BASE_URL:-none}"
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
  echo "--- canonical compatibility preflight ---"
  BASE_URL="$BASE_URL" ENV_FILE="$ENV_FILE" CHANNEL="$CH" "$SCRIPT_DIR/check_relay_compatibility.sh"
  if [[ -n "$PUBLIC_BASE_URL" ]]; then
    BASE_URL="$PUBLIC_BASE_URL" ENV_FILE="$ENV_FILE" CHANNEL="$CH" "$SCRIPT_DIR/check_relay_compatibility.sh"
  fi
  echo "--- push/pull sanity ---"
  curl -sS -D- -o /dev/null -H "X-QSL-Route-Token: $CH" "$BASE_URL/v1/pull?max=1" | sed -n '1,25p'
  printf hello | curl -sS -D- -o /dev/null -X POST -H "X-QSL-Route-Token: $CH" "$BASE_URL/v1/push" --data-binary @- | sed -n '1,25p'
  curl -sS -D- -X GET -H "X-QSL-Route-Token: $CH" "$BASE_URL/v1/pull?max=1" | sed -n '1,80p'
} | tee "$LOG"

echo "WROTE $LOG"
