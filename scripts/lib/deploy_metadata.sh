#!/usr/bin/env bash

set -euo pipefail

DEPLOY_METADATA_NAME="DEPLOYMENT_INFO"

write_deploy_metadata() {
  local base_dir="$1"
  local source_kind="$2"
  local source_value="${3:-}"
  local bin_path="${base_dir}/bin/qsl-server"
  local meta_path="${base_dir}/${DEPLOY_METADATA_NAME}"
  local tmp_path="${meta_path}.tmp"
  local sha=""

  if [[ -f "$bin_path" ]]; then
    sha="$(sha256sum "$bin_path" | awk '{print $1}')"
  fi

  {
    printf 'DEPLOYED_AT_UTC=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'DEPLOY_SOURCE_KIND=%s\n' "$source_kind"
    printf 'DEPLOY_SOURCE_VALUE=%s\n' "$source_value"
    printf 'DEPLOY_BINARY_PATH=%s\n' "$bin_path"
    printf 'DEPLOY_BINARY_SHA256=%s\n' "$sha"
    printf 'SERVICE_PATH=%s\n' "/etc/systemd/system/qsl-server.service"
    printf 'ENV_PATH=%s\n' "/etc/qsl-server/relay.env"
  } > "$tmp_path"
  chmod 0644 "$tmp_path"
  mv "$tmp_path" "$meta_path"
}
