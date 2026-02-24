#!/usr/bin/env bash
set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "ERROR: run as root" >&2
  exit 1
fi

USER_NAME="qslrelay"
GROUP_NAME="qslrelay"
BASE_DIR="/opt/qsl-server"
BIN_DIR="$BASE_DIR/bin"
ETC_DIR="/etc/qsl-server"
SERVICE_DST="/etc/systemd/system/qsl-server.service"
ENV_DST="$ETC_DIR/relay.env"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SERVICE_SRC="$REPO_ROOT/packaging/systemd/qsl-server.service"
ENV_SRC="$REPO_ROOT/packaging/systemd/relay.env.example"

BIN_SRC="${1:-$REPO_ROOT/target/release/qsl-server}"
if [[ ! -f "$BIN_SRC" ]]; then
  echo "ERROR: binary not found at $BIN_SRC" >&2
  echo "Build first: cargo build --release" >&2
  exit 1
fi

if ! getent group "$GROUP_NAME" >/dev/null; then
  groupadd --system "$GROUP_NAME"
fi
if ! id -u "$USER_NAME" >/dev/null 2>&1; then
  useradd --system --no-create-home --home-dir "$BASE_DIR" --shell /usr/sbin/nologin --gid "$GROUP_NAME" "$USER_NAME"
fi

install -d -m 0755 -o root -g root "$BASE_DIR" "$BIN_DIR" "$ETC_DIR"
install -m 0755 -o root -g root "$BIN_SRC" "$BIN_DIR/qsl-server"
install -m 0644 -o root -g root "$SERVICE_SRC" "$SERVICE_DST"

if [[ ! -f "$ENV_DST" ]]; then
  install -m 0600 -o root -g root "$ENV_SRC" "$ENV_DST"
else
  chown root:root "$ENV_DST"
  chmod 0600 "$ENV_DST"
fi

systemctl daemon-reload
systemctl enable qsl-server
systemctl restart qsl-server
systemctl --no-pager --full status qsl-server
