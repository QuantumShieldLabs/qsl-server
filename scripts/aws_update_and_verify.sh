#!/usr/bin/env bash
set -euo pipefail

ERR_USAGE="QSL_AWS_UPDATE_ERR_USAGE"
ERR_PREFLIGHT="QSL_AWS_UPDATE_ERR_PREFLIGHT"
ERR_BACKUP="QSL_AWS_UPDATE_ERR_BACKUP"
ERR_UPDATE="QSL_AWS_UPDATE_ERR_UPDATE"
ERR_VERIFY="QSL_AWS_UPDATE_ERR_VERIFY"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UPDATE_SCRIPT="${SCRIPT_DIR}/update_from_release.sh"

BASE_DIR="/opt/qsl-server"
BACKUP_DIR=""
REPO="QuantumShieldLabs/qsl-server"
RELEASE_TAG=""
ARTIFACT_URL=""
CHECKSUM_URL=""
NO_SYSTEMCTL=0
CI_MODE=0
ENV_FILE="/etc/qsl-server/relay.env"
CADDY_FILE="/etc/caddy/Caddyfile"
CADDY_LOG="/var/log/caddy/qsl-access.log"

ARTIFACT_NAME="qsl-server-linux-x86_64"
BIN_NAME="qsl-server"

usage() {
  cat <<'USAGE'
Usage:
  aws_update_and_verify.sh --release <tag> [--repo <owner/repo>] [--base-dir <dir>] [--backup-dir <dir>] [--no-systemctl]
  aws_update_and_verify.sh --artifact-url <url> --checksum-url <url> [--base-dir <dir>] [--backup-dir <dir>] [--no-systemctl]

Options:
  --ci-mode      Skip service/network checks intended for AWS host validation.
USAGE
}

mark_step() {
  local step="$1"
  local status="$2"
  echo "QSL_AWS_UPDATE_STEP=${step} status=${status}"
}

result_and_exit() {
  local outcome="$1"
  local code="$2"
  echo "QSL_AWS_UPDATE_RESULT ${outcome} code=${code}"
  if [[ "$outcome" == "PASS" ]]; then
    exit 0
  fi
  exit 1
}

fail() {
  local step="$1"
  local code="$2"
  mark_step "$step" "fail"
  result_and_exit "FAIL" "$code"
}

need_cmd() {
  local cmd="$1"
  command -v "$cmd" >/dev/null 2>&1 || fail "preflight" "${ERR_PREFLIGHT}_${cmd}_missing"
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

normalize_checksum() {
  local src="$1"
  local dst="$2"

  mapfile -t checksum_lines < "$src"
  if [[ "${#checksum_lines[@]}" -ne 1 ]]; then
    fail "post_verify_checksum" "${ERR_VERIFY}_checksum_multiline"
  fi

  local checksum_line="${checksum_lines[0]}"
  if ! [[ "$checksum_line" =~ ^([0-9a-fA-F]{64})[[:space:]]+(.+)$ ]]; then
    fail "post_verify_checksum" "${ERR_VERIFY}_checksum_format"
  fi

  local checksum_hex="${BASH_REMATCH[1]}"
  local checksum_name_raw="${BASH_REMATCH[2]}"
  local checksum_name="${checksum_name_raw##*/}"
  if [[ "$checksum_name" != "$ARTIFACT_NAME" ]]; then
    fail "post_verify_checksum" "${ERR_VERIFY}_checksum_name"
  fi

  printf '%s  %s\n' "$checksum_hex" "$ARTIFACT_NAME" > "$dst"
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
    --backup-dir)
      BACKUP_DIR="${2:-}"
      shift 2
      ;;
    --no-systemctl)
      NO_SYSTEMCTL=1
      shift
      ;;
    --ci-mode)
      CI_MODE=1
      shift
      ;;
    --env-file)
      ENV_FILE="${2:-}"
      shift 2
      ;;
    --caddy-file)
      CADDY_FILE="${2:-}"
      shift 2
      ;;
    --caddy-log)
      CADDY_LOG="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      fail "args" "${ERR_USAGE}_unknown_arg"
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
  fail "args" "${ERR_USAGE}_missing_update_source"
fi

if [[ -z "$BACKUP_DIR" ]]; then
  BACKUP_DIR="/root/qsl-backups/$(date -u +%Y%m%dT%H%M%SZ)"
fi

BIN_PATH="${BASE_DIR}/bin/${BIN_NAME}"
TMP_DIR="$(mktemp -d)"
CHECKSUM_RAW="${TMP_DIR}/${ARTIFACT_NAME}.sha256"
CHECKSUM_NORMALIZED="${TMP_DIR}/${ARTIFACT_NAME}.normalized.sha256"
INSTALLED_COPY="${TMP_DIR}/${ARTIFACT_NAME}"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

# 1) Preflight
need_cmd bash
need_cmd curl
need_cmd sha256sum
need_cmd ss
[[ "$NO_SYSTEMCTL" -eq 1 ]] || need_cmd systemctl
[[ -x "$UPDATE_SCRIPT" ]] || fail "preflight" "${ERR_PREFLIGHT}_update_script_missing"
mark_step "preflight" "ok"

# 2) Backup
if ! mkdir -p "$BACKUP_DIR"; then
  fail "backup" "${ERR_BACKUP}_mkdir"
fi

if [[ -f "$ENV_FILE" ]]; then
  cp "$ENV_FILE" "$BACKUP_DIR/relay.env" || fail "backup" "${ERR_BACKUP}_relay_env"
  awk -F= '/^[A-Z0-9_]+=/{print $1}' "$ENV_FILE" > "$BACKUP_DIR/relay.env.keys" || true
fi

if [[ -f "$CADDY_FILE" ]]; then
  cp "$CADDY_FILE" "$BACKUP_DIR/Caddyfile" || fail "backup" "${ERR_BACKUP}_caddyfile"
fi

if [[ "$NO_SYSTEMCTL" -eq 0 ]]; then
  systemctl cat qsl-server > "$BACKUP_DIR/qsl-server.unit.txt" || fail "backup" "${ERR_BACKUP}_unit"
fi

if [[ -f "$BIN_PATH" ]]; then
  cp "$BIN_PATH" "$BACKUP_DIR/qsl-server.bin" || fail "backup" "${ERR_BACKUP}_binary"
  sha256sum "$BACKUP_DIR/qsl-server.bin" > "$BACKUP_DIR/qsl-server.bin.sha256" || fail "backup" "${ERR_BACKUP}_binary_sha"
fi
mark_step "backup" "ok"

# 3) Run checksum-verified update
update_args=("--base-dir" "$BASE_DIR")
[[ "$NO_SYSTEMCTL" -eq 1 ]] && update_args+=("--no-systemctl")
if [[ -n "$RELEASE_TAG" ]]; then
  update_args+=("--release" "$RELEASE_TAG" "--repo" "$REPO")
else
  update_args+=("--artifact-url" "$ARTIFACT_URL" "--checksum-url" "$CHECKSUM_URL")
fi

if ! bash "$UPDATE_SCRIPT" "${update_args[@]}"; then
  fail "update" "${ERR_UPDATE}_updater_failed"
fi
mark_step "update" "ok"

# 4) Post-update verification
# checksum verification against source checksum
if ! download_file "$CHECKSUM_URL" "$CHECKSUM_RAW"; then
  fail "post_verify_checksum" "${ERR_VERIFY}_checksum_download"
fi
normalize_checksum "$CHECKSUM_RAW" "$CHECKSUM_NORMALIZED"

[[ -f "$BIN_PATH" ]] || fail "post_verify_checksum" "${ERR_VERIFY}_binary_missing"
cp "$BIN_PATH" "$INSTALLED_COPY" || fail "post_verify_checksum" "${ERR_VERIFY}_binary_copy"
if ! (cd "$TMP_DIR" && sha256sum -c "$CHECKSUM_NORMALIZED" >/dev/null); then
  fail "post_verify_checksum" "${ERR_VERIFY}_checksum_mismatch"
fi
mark_step "post_verify_checksum" "ok"

if [[ "$CI_MODE" -eq 0 ]]; then
  if [[ -f "$ENV_FILE" ]]; then
    env_meta="$(stat -c '%a %U %G' "$ENV_FILE" 2>/dev/null || true)"
    [[ "$env_meta" == "600 root root" ]] || fail "post_verify_env_perms" "${ERR_VERIFY}_env_perms"
  else
    fail "post_verify_env_perms" "${ERR_VERIFY}_env_missing"
  fi
  mark_step "post_verify_env_perms" "ok"

  port="8080"
  if [[ -f "$ENV_FILE" ]]; then
    env_port="$(awk -F= '$1=="PORT"{print $2}' "$ENV_FILE" | tail -n 1 | tr -d '[:space:]' || true)"
    if [[ -n "$env_port" ]]; then
      port="$env_port"
    fi
  fi

  if ! ss -lnt "( sport = :${port} )" | awk 'NR>1 {print $4}' | rg -q "(^127\\.0\\.0\\.1:${port}$|^\[::1\]:${port}$)"; then
    fail "post_verify_loopback_bind" "${ERR_VERIFY}_loopback_bind"
  fi
  mark_step "post_verify_loopback_bind" "ok"

  local_status="$(curl -sS -o /dev/null -w '%{http_code}' --max-time 5 "http://127.0.0.1:${port}/v1/pull/qsc-selftest?max=1" || true)"
  echo "QSL_AWS_UPDATE_STEP=post_verify_local_http status_code=${local_status:-000}"
  [[ "${local_status:-000}" == "401" ]] || fail "post_verify_local_http" "${ERR_VERIFY}_local_http_status"
  mark_step "post_verify_local_http" "ok"

  caddy_host=""
  if [[ -f "$CADDY_FILE" ]]; then
    caddy_host="$(awk '
      /^[[:space:]]*#/ {next}
      /^[[:space:]]*$/ {next}
      /\{/ {
        token=$1
        gsub(/[,{]/, "", token)
        if (token != "" && token !~ /^@/) { print token; exit }
      }
    ' "$CADDY_FILE" || true)"
  fi

  if [[ -n "$caddy_host" ]]; then
    https_status="$(curl -sk -o /dev/null -w '%{http_code}' --max-time 8 "https://${caddy_host}/v1/pull/qsc-selftest?max=1" || true)"
    echo "QSL_AWS_UPDATE_STEP=post_verify_https status_code=${https_status:-000}"
    [[ "${https_status:-000}" == "401" ]] || fail "post_verify_https" "${ERR_VERIFY}_https_status"
    mark_step "post_verify_https" "ok"
  else
    echo "QSL_AWS_UPDATE_STEP=post_verify_https status=skip code=no_caddy_host"
  fi

  if [[ -f "$CADDY_FILE" ]]; then
    rg -q "path /v1/\\*" "$CADDY_FILE" || fail "post_verify_caddy_hygiene" "${ERR_VERIFY}_missing_v1_matcher"
    rg -q "log_skip" "$CADDY_FILE" || fail "post_verify_caddy_hygiene" "${ERR_VERIFY}_missing_log_skip"
  else
    fail "post_verify_caddy_hygiene" "${ERR_VERIFY}_caddyfile_missing"
  fi

  if [[ -f "$CADDY_LOG" ]]; then
    before_count="$(rg -c '/v1/' "$CADDY_LOG" || true)"
    curl -sk -o /dev/null --max-time 8 "https://${caddy_host}/v1/pull/qsc-selftest?max=1" || true
    after_count="$(rg -c '/v1/' "$CADDY_LOG" || true)"
    echo "QSL_AWS_UPDATE_STEP=post_verify_caddy_log_delta before=${before_count:-0} after=${after_count:-0}"
    [[ "${before_count:-0}" == "${after_count:-0}" ]] || fail "post_verify_caddy_hygiene" "${ERR_VERIFY}_v1_log_delta"
  else
    echo "QSL_AWS_UPDATE_STEP=post_verify_caddy_log_delta status=skip code=NO_LOG_FILE"
  fi
  mark_step "post_verify_caddy_hygiene" "ok"
else
  echo "QSL_AWS_UPDATE_STEP=post_verify_env_perms status=skip code=ci_mode"
  echo "QSL_AWS_UPDATE_STEP=post_verify_loopback_bind status=skip code=ci_mode"
  echo "QSL_AWS_UPDATE_STEP=post_verify_local_http status=skip code=ci_mode"
  echo "QSL_AWS_UPDATE_STEP=post_verify_https status=skip code=ci_mode"
  echo "QSL_AWS_UPDATE_STEP=post_verify_caddy_hygiene status=skip code=ci_mode"
fi

result_and_exit "PASS" "ok"
