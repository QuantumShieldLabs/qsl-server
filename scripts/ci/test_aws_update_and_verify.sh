#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
WRAPPER_SCRIPT="${REPO_ROOT}/scripts/aws_update_and_verify.sh"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

BASE_DIR="${TMP_DIR}/base"
ENV_FILE="${TMP_DIR}/relay.env"
CADDY_FILE="${TMP_DIR}/Caddyfile"
CADDY_LOG="${TMP_DIR}/qsl-access.log"

mkdir -p "${BASE_DIR}/bin"
printf 'old-binary\n' > "${BASE_DIR}/bin/qsl-server"
chmod 0755 "${BASE_DIR}/bin/qsl-server"
cp "${BASE_DIR}/bin/qsl-server" "${TMP_DIR}/old.copy"

cat > "$ENV_FILE" <<'ENV'
PORT=8080
BIND_ADDR=127.0.0.1
RELAY_TOKEN=
ENV
chmod 600 "$ENV_FILE"

cat > "$CADDY_FILE" <<'CADDY'
example.invalid {
  @qsl_v1 path /v1/*
  log_skip @qsl_v1
}
CADDY
: > "$CADDY_LOG"

printf 'new-binary\n' > "${TMP_DIR}/qsl-server-linux-x86_64"
(cd "$TMP_DIR" && sha256sum qsl-server-linux-x86_64 > qsl-server-linux-x86_64.sha256)

pass_log="${TMP_DIR}/pass.log"
bash "$WRAPPER_SCRIPT" \
  --artifact-url "file://${TMP_DIR}/qsl-server-linux-x86_64" \
  --checksum-url "file://${TMP_DIR}/qsl-server-linux-x86_64.sha256" \
  --base-dir "$BASE_DIR" \
  --backup-dir "${TMP_DIR}/backup-pass" \
  --no-systemctl \
  --ci-mode \
  --env-file "$ENV_FILE" \
  --caddy-file "$CADDY_FILE" \
  --caddy-log "$CADDY_LOG" \
  > "$pass_log" 2>&1

rg -q "QSL_AWS_UPDATE_RESULT PASS code=ok" "$pass_log"
if ! grep -q "new-binary" "${BASE_DIR}/bin/qsl-server"; then
  echo "expected updated binary content on success path"
  exit 1
fi

printf 'old-binary\n' > "${BASE_DIR}/bin/qsl-server"
cp "${BASE_DIR}/bin/qsl-server" "${TMP_DIR}/old-fail.copy"
echo "0000000000000000000000000000000000000000000000000000000000000000  qsl-server-linux-x86_64" > "${TMP_DIR}/qsl-server-linux-x86_64.sha256"

fail_log="${TMP_DIR}/fail.log"
set +e
bash "$WRAPPER_SCRIPT" \
  --artifact-url "file://${TMP_DIR}/qsl-server-linux-x86_64" \
  --checksum-url "file://${TMP_DIR}/qsl-server-linux-x86_64.sha256" \
  --base-dir "$BASE_DIR" \
  --backup-dir "${TMP_DIR}/backup-fail" \
  --no-systemctl \
  --ci-mode \
  --env-file "$ENV_FILE" \
  --caddy-file "$CADDY_FILE" \
  --caddy-log "$CADDY_LOG" \
  > "$fail_log" 2>&1
rc=$?
set -e

if [[ "$rc" -eq 0 ]]; then
  echo "expected mismatch checksum path to fail"
  exit 1
fi

rg -q "QSL_AWS_UPDATE_RESULT FAIL code=" "$fail_log"
cmp "${BASE_DIR}/bin/qsl-server" "${TMP_DIR}/old-fail.copy"

echo "aws update wrapper CI test passed"
