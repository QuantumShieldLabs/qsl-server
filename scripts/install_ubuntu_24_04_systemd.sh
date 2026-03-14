#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cat >&2 <<'EOF'
DEPRECATED: scripts/install_ubuntu_24_04_systemd.sh is no longer the canonical install path.
Use scripts/install_ubuntu.sh with a built qsl-server binary instead.
This wrapper now delegates to the canonical installer and no longer clones/builds the repo on-host.
EOF

exec bash "$REPO_ROOT/scripts/install_ubuntu.sh" "${1:-$REPO_ROOT/target/release/qsl-server}"
