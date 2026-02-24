#!/usr/bin/env bash
set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "ERROR: run as root" >&2
  exit 1
fi

BASE_DIR="/opt/qsl-server"
BIN_DIR="$BASE_DIR/bin"
BIN_DST="$BIN_DIR/qsl-server"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BIN_SRC="${1:-$REPO_ROOT/target/release/qsl-server}"

if [[ ! -f "$BIN_SRC" ]]; then
  echo "ERROR: binary not found at $BIN_SRC" >&2
  echo "Build first: cargo build --release" >&2
  exit 1
fi

install -d -m 0755 -o root -g root "$BASE_DIR" "$BIN_DIR"

systemctl stop qsl-server
install -m 0755 -o root -g root "$BIN_SRC" "$BIN_DST"
systemctl start qsl-server
systemctl --no-pager --full status qsl-server
