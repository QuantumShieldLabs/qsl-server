#!/usr/bin/env bash
set -euo pipefail

ERR_USAGE="QSL_UPDATE_ERR_USAGE"
ERR_DOWNLOAD="QSL_UPDATE_ERR_DOWNLOAD"
ERR_CHECKSUM="QSL_UPDATE_ERR_CHECKSUM_MISMATCH"
ERR_INPUT="QSL_UPDATE_ERR_INPUT"
ERR_INSTALL="QSL_UPDATE_ERR_INSTALL"

BASE_DIR="/opt/qsl-server"
BIN_NAME="qsl-server"
ARTIFACT_NAME="qsl-server-linux-x86_64"
REPO="QuantumShieldLabs/qsl-server"
NO_SYSTEMCTL=0
RELEASE_TAG=""
ARTIFACT_URL=""
CHECKSUM_URL=""

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/deploy_metadata.sh"

usage() {
  cat <<'EOF'
Usage:
  update_from_release.sh --release <tag> [--repo <owner/repo>] [--base-dir <dir>] [--no-systemctl]
  update_from_release.sh --artifact-url <url> --checksum-url <url> [--base-dir <dir>] [--no-systemctl]
EOF
}

fail() {
  local code="$1"
  local msg="$2"
  echo "${code}: ${msg}" >&2
  exit 1
}

download_file() {
  local src="$1"
  local dst="$2"
  if [[ "$src" == file://* ]]; then
    cp "${src#file://}" "$dst"
  else
    curl -fsSL "$src" -o "$dst"
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      RELEASE_TAG="${2:-}"
      shift 2
      ;;
    --repo)
      REPO="${2:-}"
      shift 2
      ;;
    --artifact-url)
      ARTIFACT_URL="${2:-}"
      shift 2
      ;;
    --checksum-url)
      CHECKSUM_URL="${2:-}"
      shift 2
      ;;
    --base-dir)
      BASE_DIR="${2:-}"
      shift 2
      ;;
    --no-systemctl)
      NO_SYSTEMCTL=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      fail "$ERR_USAGE" "unknown argument: $1"
      ;;
  esac
done

if [[ -n "$RELEASE_TAG" ]]; then
  base_url="https://github.com/${REPO}/releases/download/${RELEASE_TAG}"
  ARTIFACT_URL="${ARTIFACT_URL:-${base_url}/${ARTIFACT_NAME}}"
  CHECKSUM_URL="${CHECKSUM_URL:-${base_url}/${ARTIFACT_NAME}.sha256}"
fi

if [[ -z "$ARTIFACT_URL" || -z "$CHECKSUM_URL" ]]; then
  usage
  fail "$ERR_USAGE" "must provide either --release <tag> or explicit --artifact-url and --checksum-url"
fi

BIN_DIR="${BASE_DIR}/bin"
BIN_DST="${BIN_DIR}/${BIN_NAME}"
TMP_DIR="$(mktemp -d)"
ARTIFACT_TMP="${TMP_DIR}/${ARTIFACT_NAME}"
CHECKSUM_TMP="${TMP_DIR}/${ARTIFACT_NAME}.sha256"
CHECKSUM_NORMALIZED="${TMP_DIR}/${ARTIFACT_NAME}.normalized.sha256"
BACKUP_TMP="${TMP_DIR}/${BIN_NAME}.backup"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

install -d -m 0755 "$BIN_DIR"

download_file "$ARTIFACT_URL" "$ARTIFACT_TMP" || fail "$ERR_DOWNLOAD" "failed to download artifact"
download_file "$CHECKSUM_URL" "$CHECKSUM_TMP" || fail "$ERR_DOWNLOAD" "failed to download checksum"

mapfile -t checksum_lines < "$CHECKSUM_TMP"
if [[ "${#checksum_lines[@]}" -ne 1 ]]; then
  fail "$ERR_INPUT" "checksum file must contain exactly one line"
fi

checksum_line="${checksum_lines[0]}"
if ! [[ "$checksum_line" =~ ^([0-9a-fA-F]{64})[[:space:]]+(.+)$ ]]; then
  fail "$ERR_INPUT" "checksum file format invalid"
fi

checksum_hex="${BASH_REMATCH[1]}"
checksum_name_raw="${BASH_REMATCH[2]}"
checksum_name="${checksum_name_raw##*/}"
if [[ "$checksum_name" != "$ARTIFACT_NAME" ]]; then
  fail "$ERR_INPUT" "checksum file references unsupported artifact"
fi

printf '%s  %s\n' "$checksum_hex" "$ARTIFACT_NAME" > "$CHECKSUM_NORMALIZED"

if ! (cd "$TMP_DIR" && sha256sum -c "$CHECKSUM_NORMALIZED" >/dev/null); then
  fail "$ERR_CHECKSUM" "checksum verification failed"
fi

chmod 0755 "$ARTIFACT_TMP"

if [[ "$NO_SYSTEMCTL" -eq 0 ]]; then
  systemctl stop qsl-server
fi

if [[ -f "$BIN_DST" ]]; then
  mv "$BIN_DST" "$BACKUP_TMP"
fi

if ! mv "$ARTIFACT_TMP" "$BIN_DST"; then
  if [[ -f "$BACKUP_TMP" ]]; then
    mv "$BACKUP_TMP" "$BIN_DST" || true
  fi
  fail "$ERR_INSTALL" "failed to install new binary"
fi

if [[ "$NO_SYSTEMCTL" -eq 0 ]]; then
  if ! systemctl start qsl-server; then
    if [[ -f "$BACKUP_TMP" ]]; then
      mv "$BACKUP_TMP" "$BIN_DST" || true
      systemctl start qsl-server || true
    fi
    fail "$ERR_INSTALL" "service failed to start after install"
  fi
  systemctl --no-pager --full status qsl-server
fi

deploy_source_kind="custom_artifact"
deploy_source_value="$ARTIFACT_NAME"
if [[ -n "$RELEASE_TAG" ]]; then
  deploy_source_kind="release_tag"
  deploy_source_value="$RELEASE_TAG"
fi
write_deploy_metadata "$BASE_DIR" "$deploy_source_kind" "$deploy_source_value"
