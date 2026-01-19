#!/usr/bin/env bash
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  echo "ERROR: run as root" >&2
  exit 1
fi

REPO_URL="https://github.com/QuantumShieldLabs/qsl-server.git"
BASE="/opt/qsl-server"
REPO_DIR="$BASE/repo"
BIN_DIR="$BASE/bin"
CARGO_HOME="$BASE/.cargo"
RUSTUP_HOME="$BASE/.rustup"
CARGO_TARGET_DIR="$REPO_DIR/target"
USER="qslrelay"
GROUP="qslrelay"

apt-get update -y
apt-get install -y git curl ca-certificates

if ! id -u "$USER" >/dev/null 2>&1; then
  useradd --system --create-home --home-dir "$BASE" --shell /usr/sbin/nologin "$USER"
fi

mkdir -p "$BASE" "$BIN_DIR" "$REPO_DIR" /var/log/qsl-server
chown -R "$USER:$GROUP" "$BASE" /var/log/qsl-server

if [ -d "$REPO_DIR/.git" ]; then
  sudo -u "$USER" -H git -C "$REPO_DIR" fetch --all
  sudo -u "$USER" -H git -C "$REPO_DIR" reset --hard origin/main
else
  sudo -u "$USER" -H git clone "$REPO_URL" "$REPO_DIR"
fi

if [ ! -d "$RUSTUP_HOME/toolchains" ]; then
  sudo -u "$USER" -H env CARGO_HOME="$CARGO_HOME" RUSTUP_HOME="$RUSTUP_HOME" \
    sh -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal'
fi

sudo -u "$USER" -H env CARGO_HOME="$CARGO_HOME" RUSTUP_HOME="$RUSTUP_HOME" CARGO_TARGET_DIR="$CARGO_TARGET_DIR" \
  bash -lc 'cd /opt/qsl-server/repo && cargo build --release'

ln -sf "$CARGO_TARGET_DIR/release/qsl-server" "$BIN_DIR/qsl-server"

install -m 0644 "$REPO_DIR/systemd/qsl-server.service" /etc/systemd/system/qsl-server.service
systemctl daemon-reload
systemctl enable --now qsl-server
systemctl status qsl-server --no-pager
