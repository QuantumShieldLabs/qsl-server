#!/usr/bin/env bash
set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "ERROR: run as root" >&2
  exit 1
fi

BASE_DIR="/opt/qsl-server"
BIN_DIR="$BASE_DIR/bin"
BIN_DST="$BIN_DIR/qsl-server"
ETC_DIR="/etc/qsl-server"
SERVICE_DST="/etc/systemd/system/qsl-server.service"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
source "$REPO_ROOT/scripts/lib/deploy_metadata.sh"
SERVICE_SRC="$REPO_ROOT/packaging/systemd/qsl-server.service"
ENV_SRC="$REPO_ROOT/packaging/systemd/relay.env.example"
ENV_DST="$ETC_DIR/relay.env"
BIN_SRC="${1:-$REPO_ROOT/target/release/qsl-server}"

if [[ ! -f "$BIN_SRC" ]]; then
  echo "ERROR: binary not found at $BIN_SRC" >&2
  echo "Build first: cargo build --release" >&2
  exit 1
fi

install -d -m 0755 -o root -g root "$BASE_DIR" "$BIN_DIR" "$ETC_DIR"
install -m 0644 -o root -g root "$SERVICE_SRC" "$SERVICE_DST"

if [[ ! -f "$ENV_DST" ]]; then
  install -m 0600 -o root -g root "$ENV_SRC" "$ENV_DST"
else
  chown root:root "$ENV_DST"
  chmod 0600 "$ENV_DST"
fi

systemctl stop qsl-server
install -m 0755 -o root -g root "$BIN_SRC" "$BIN_DST"
write_deploy_metadata "$BASE_DIR" "local_binary" "$BIN_SRC"
systemctl daemon-reload
systemctl start qsl-server
systemctl --no-pager --full status qsl-server
