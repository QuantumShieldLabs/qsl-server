#!/usr/bin/env bash
set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "ERROR: run as root" >&2
  exit 1
fi

echo "== qsl relay audit =="
echo "timestamp: $(date -u +%Y-%m-%dT%H:%M:%SZ)"

echo
echo "[os]"
uname -a

ENV_FILE="/etc/qsl-server/relay.env"
CADDY_CONF_DEFAULT="/etc/caddy/Caddyfile"

echo
echo "[service] qsl-server"
systemctl --no-pager --full status qsl-server || true

echo
echo "[listeners] ports 80/443/8080"
ss -lntp | grep -E '(:80\b|:443\b|:8080\b)' || true

echo
echo "[env file perms]"
if [[ -f "$ENV_FILE" ]]; then
  stat -c '%n %a %U:%G' "$ENV_FILE"
  token_line="$(grep -E '^RELAY_TOKEN=' "$ENV_FILE" || true)"
  token_value="${token_line#RELAY_TOKEN=}"
  if [[ -n "$token_line" && -n "$token_value" ]]; then
    echo "RELAY_TOKEN: set"
  else
    echo "RELAY_TOKEN: unset"
  fi
else
  echo "$ENV_FILE missing"
fi

echo
echo "[bind posture]"
if ss -lntp | grep -E '127\.0\.0\.1:8080|\[::1\]:8080' >/dev/null 2>&1; then
  echo "bind posture: loopback (safe default)"
elif ss -lntp | grep -E '0\.0\.0\.0:8080|\[::\]:8080' >/dev/null 2>&1; then
  echo "bind posture: public/any (explicit opt-in expected)"
else
  echo "bind posture: no 8080 listener detected"
fi

echo
echo "[service] caddy"
systemctl --no-pager --full status caddy || true

caddy_fragment="$(systemctl show -p FragmentPath --value caddy 2>/dev/null || true)"
if [[ -n "$caddy_fragment" ]]; then
  echo "caddy fragment path: $caddy_fragment"
else
  echo "caddy fragment path: unknown"
fi

caddy_conf="${CADDYFILE_PATH:-$CADDY_CONF_DEFAULT}"
echo "caddy config path checked: $caddy_conf"
if [[ -f "$caddy_conf" ]]; then
  if grep -E 'path\s+/v1/\*' "$caddy_conf" >/dev/null 2>&1 \
    && grep -E 'log_skip' "$caddy_conf" >/dev/null 2>&1; then
    echo "v1 log hygiene: looks configured (matcher + log_skip found)"
  else
    echo "v1 log hygiene: NOT confirmed (missing matcher and/or log_skip)"
  fi
else
  echo "caddy config file not found"
fi
