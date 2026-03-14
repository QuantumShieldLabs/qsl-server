#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

README_FILE="${REPO_ROOT}/README.md"
RUNBOOK_FILE="${REPO_ROOT}/packaging/runbook_ubuntu.md"
VERIFY_FILE="${REPO_ROOT}/scripts/verify_remote.sh"
AWS_VERIFY_FILE="${REPO_ROOT}/scripts/aws_update_and_verify.sh"

rg -q 'POST /v1/push' "$README_FILE"
rg -q 'GET /v1/pull\?max=N' "$README_FILE"
rg -q 'X-QSL-Route-Token' "$README_FILE"

rg -q 'X-QSL-Route-Token: audit' "$RUNBOOK_FILE"
! rg -q '/v1/pull/audit\?max=1' "$RUNBOOK_FILE"

rg -q 'X-QSL-Route-Token: \$CH' "$VERIFY_FILE"
! rg -q '/v1/pull/\$CH' "$VERIFY_FILE"
! rg -q '/v1/push/\$CH' "$VERIFY_FILE"

rg -q 'X-QSL-Route-Token: qsc-selftest' "$AWS_VERIFY_FILE"
! rg -q '/v1/pull/qsc-selftest' "$AWS_VERIFY_FILE"

echo "route-token migration operator examples check passed"
