#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

assert_contains() {
  local path="$1"
  local pattern="$2"
  if ! rg -q --fixed-strings "$pattern" "$path"; then
    echo "missing expected pattern in ${path}: ${pattern}" >&2
    exit 1
  fi
}

assert_not_contains() {
  local path="$1"
  local pattern="$2"
  if rg -q --fixed-strings "$pattern" "$path"; then
    echo "unexpected pattern in ${path}: ${pattern}" >&2
    exit 1
  fi
}

assert_not_match() {
  local path="$1"
  local pattern="$2"
  if rg -q "$pattern" "$path"; then
    echo "unexpected regex match in ${path}: ${pattern}" >&2
    exit 1
  fi
}

assert_contains "$REPO_ROOT/README.md" "packaging/systemd/qsl-server.service"
assert_contains "$REPO_ROOT/README.md" "packaging/systemd/relay.env.example"
assert_contains "$REPO_ROOT/packaging/runbook_ubuntu.md" "/etc/qsl-server/relay.env"
assert_contains "$REPO_ROOT/packaging/systemd/qsl-server.service" "EnvironmentFile=/etc/qsl-server/relay.env"
assert_contains "$REPO_ROOT/scripts/install_ubuntu.sh" "packaging/systemd/qsl-server.service"
assert_contains "$REPO_ROOT/scripts/install_ubuntu.sh" "packaging/systemd/relay.env.example"
assert_contains "$REPO_ROOT/scripts/update_ubuntu.sh" "packaging/systemd/qsl-server.service"
assert_contains "$REPO_ROOT/scripts/verify_remote.sh" "DEPLOYMENT_INFO"
assert_contains "$REPO_ROOT/scripts/install_ubuntu_24_04_systemd.sh" "DEPRECATED:"
assert_contains "$REPO_ROOT/scripts/install_ubuntu_24_04_systemd.sh" "scripts/install_ubuntu.sh"

assert_not_contains "$REPO_ROOT/README.md" "install_ubuntu_24_04_systemd.sh"
assert_not_contains "$REPO_ROOT/packaging/runbook_ubuntu.md" "install_ubuntu_24_04_systemd.sh"
assert_not_contains "$REPO_ROOT/scripts/verify_remote.sh" "/opt/qsl-server/repo"
assert_not_contains "$REPO_ROOT/scripts/install_ubuntu_24_04_systemd.sh" "/opt/qsl-server/repo"
assert_not_contains "$REPO_ROOT/scripts/install_ubuntu_24_04_systemd.sh" "cargo build --release"
assert_not_match "$REPO_ROOT/README.md" '(^|[^[:alnum:]/])systemd/qsl-server\.service([^[:alnum:]]|$)'
assert_not_match "$REPO_ROOT/packaging/runbook_ubuntu.md" '(^|[^[:alnum:]/])systemd/qsl-server\.service([^[:alnum:]]|$)'

if [[ -e "$REPO_ROOT/systemd/qsl-server.service" ]]; then
  echo "legacy root systemd/qsl-server.service should be removed" >&2
  exit 1
fi

echo "canonical packaging alignment test passed"
