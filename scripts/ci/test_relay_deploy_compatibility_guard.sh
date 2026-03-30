#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"; if [[ -n "${SERVER_PID:-}" ]]; then kill "$SERVER_PID" >/dev/null 2>&1 || true; fi' EXIT

PORT="$(python3 - <<'PY'
import socket

sock = socket.socket()
sock.bind(("127.0.0.1", 0))
print(sock.getsockname()[1])
sock.close()
PY
)"

python3 - "$PORT" <<'PY' &
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def do_GET(self):
        if self.path.startswith("/v1/pull?"):
            self.send_response(204)
        elif self.path.startswith("/v1/pull/"):
            self.send_response(204)
        else:
            self.send_response(500)
        self.end_headers()


HTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
PY
SERVER_PID=$!
sleep 1

set +e
BASE_URL="http://127.0.0.1:${PORT}" ENV_FILE="${TMP_DIR}/missing.env" CHANNEL="compat-probe" \
  "${REPO_ROOT}/scripts/check_relay_compatibility.sh" >"${TMP_DIR}/guard.out" 2>&1
status=$?
set -e

if [[ "$status" -eq 0 ]]; then
  echo "expected stale relay guard to fail" >&2
  exit 1
fi

rg -q 'check_relay_compatibility.sh' "${REPO_ROOT}/scripts/verify_remote.sh"
rg -q 'check_relay_compatibility.sh' "${REPO_ROOT}/scripts/aws_update_and_verify.sh"
rg -q 'legacy_path_still_enabled' "${REPO_ROOT}/scripts/check_relay_compatibility.sh"
rg -Fq '/v1/pull?max=1' "${REPO_ROOT}/scripts/check_relay_compatibility.sh"
rg -Fq '/v1/pull/${CHANNEL}?max=1' "${REPO_ROOT}/scripts/check_relay_compatibility.sh"
rg -Fq 'install -d -m 0755 "$LOG_DIR"' "${REPO_ROOT}/scripts/verify_remote.sh"
rg -q 'QSL_RELAY_COMPAT_RESULT FAIL code=legacy_path_still_enabled' "${TMP_DIR}/guard.out"
rg -Fq 'Run `scripts/verify_remote.sh` before any real-world validation' "${REPO_ROOT}/README.md"

echo "relay deployment compatibility guard check passed"
