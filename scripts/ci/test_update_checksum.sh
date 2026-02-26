#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
UPDATE_SCRIPT="${REPO_ROOT}/scripts/update_from_release.sh"

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

BASE_DIR="${TMP_DIR}/install-root"
mkdir -p "${BASE_DIR}/bin"
printf 'old-binary\n' > "${BASE_DIR}/bin/qsl-server"
chmod 0755 "${BASE_DIR}/bin/qsl-server"
cp "${BASE_DIR}/bin/qsl-server" "${TMP_DIR}/old.copy"

printf 'new-binary\n' > "${TMP_DIR}/qsl-server-linux-x86_64"
echo "0000000000000000000000000000000000000000000000000000000000000000  qsl-server-linux-x86_64" > "${TMP_DIR}/qsl-server-linux-x86_64.sha256"

set +e
bash "$UPDATE_SCRIPT" \
  --artifact-url "file://${TMP_DIR}/qsl-server-linux-x86_64" \
  --checksum-url "file://${TMP_DIR}/qsl-server-linux-x86_64.sha256" \
  --base-dir "$BASE_DIR" \
  --no-systemctl
rc=$?
set -e

if [[ "$rc" -eq 0 ]]; then
  echo "expected checksum mismatch to fail"
  exit 1
fi

cmp "${BASE_DIR}/bin/qsl-server" "${TMP_DIR}/old.copy"

(cd "${TMP_DIR}" && sha256sum "qsl-server-linux-x86_64" > "qsl-server-linux-x86_64.sha256")

bash "$UPDATE_SCRIPT" \
  --artifact-url "file://${TMP_DIR}/qsl-server-linux-x86_64" \
  --checksum-url "file://${TMP_DIR}/qsl-server-linux-x86_64.sha256" \
  --base-dir "$BASE_DIR" \
  --no-systemctl

if ! grep -q "new-binary" "${BASE_DIR}/bin/qsl-server"; then
  echo "expected updated binary content"
  exit 1
fi

printf 'old-binary\n' > "${BASE_DIR}/bin/qsl-server"
cp "${BASE_DIR}/bin/qsl-server" "${TMP_DIR}/old.copy.dist"
(cd "${TMP_DIR}" && sha256sum "qsl-server-linux-x86_64" | sed 's# qsl-server-linux-x86_64$# dist/qsl-server-linux-x86_64#' > "qsl-server-linux-x86_64.sha256")

bash "$UPDATE_SCRIPT" \
  --artifact-url "file://${TMP_DIR}/qsl-server-linux-x86_64" \
  --checksum-url "file://${TMP_DIR}/qsl-server-linux-x86_64.sha256" \
  --base-dir "$BASE_DIR" \
  --no-systemctl

if ! grep -q "new-binary" "${BASE_DIR}/bin/qsl-server"; then
  echo "expected dist/ checksum filename to pass"
  exit 1
fi

printf 'old-binary\n' > "${BASE_DIR}/bin/qsl-server"
cp "${BASE_DIR}/bin/qsl-server" "${TMP_DIR}/old.copy.evil"
(cd "${TMP_DIR}" && sha256sum "qsl-server-linux-x86_64" | sed 's# qsl-server-linux-x86_64$# dist/evil.bin#' > "qsl-server-linux-x86_64.sha256")

set +e
bash "$UPDATE_SCRIPT" \
  --artifact-url "file://${TMP_DIR}/qsl-server-linux-x86_64" \
  --checksum-url "file://${TMP_DIR}/qsl-server-linux-x86_64.sha256" \
  --base-dir "$BASE_DIR" \
  --no-systemctl
rc=$?
set -e

if [[ "$rc" -eq 0 ]]; then
  echo "expected wrong basename checksum to fail"
  exit 1
fi

cmp "${BASE_DIR}/bin/qsl-server" "${TMP_DIR}/old.copy.evil"

echo "checksum enforcement test passed"
