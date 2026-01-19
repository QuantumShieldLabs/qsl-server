#!/usr/bin/env bash
set -euo pipefail

TS="$(date -u +%Y%m%dT%H%M%SZ)"
LOG="/var/log/qsl-server/verify_${TS}.log"
BASE_URL="${BASE_URL:-http://127.0.0.1:8080}"
CH="${CHANNEL:-verify-${TS}}"

{
  echo "TS=$TS"
  echo "BASE_URL=$BASE_URL"
  echo "CHANNEL=$CH"
  echo "--- systemd status ---"
  systemctl is-active qsl-server
  systemctl status qsl-server --no-pager
  echo "--- listener ---"
  ss -ltnp | grep -E ':8080\b' || true
  echo "--- deploy head ---"
  sudo -u qslrelay -H bash -lc 'cd /opt/qsl-server/repo && printf "DEPLOYED_HEAD=%s\n" "$(git rev-parse HEAD)" && git log -1 --oneline'
  echo "--- push/pull sanity ---"
  curl -sS -D- -o /dev/null "$BASE_URL/v1/pull/$CH" | sed -n '1,25p'
  printf hello | curl -sS -D- -o /dev/null -X POST "$BASE_URL/v1/push/$CH" --data-binary @- | sed -n '1,25p'
  curl -sS -D- -X GET "$BASE_URL/v1/pull/$CH" | sed -n '1,80p'
} | tee "$LOG"

echo "WROTE $LOG"
