# NA-0011 Relay Compatibility Restore Evidence

## Scope
- qsl-server deployment, scripts, docs, and evidence only
- no `src/**`, `Cargo.toml`, `Cargo.lock`, workflow, qsl-protocol, or qsl-attachments changes

## Current-source proof
- At the NA-0011 snapshot, qsl-server `main` implemented canonical header-based carriage on `POST /v1/push` and `GET /v1/pull?max=N` while still exposing the legacy compatibility routes `POST /v1/push/:channel` and `GET /v1/pull/:channel?max=N`.
- Current qsl-server code has since retired those legacy path-token routes; they return 404 and do not mutate or consume canonical queues.
- The blocker for `NA-0200A` was deployment drift on the live relay, not missing source support in qsl-server.

## Before restore: live drift proof
- Canonical loopback probe: `404`
- Legacy loopback probe: `204`
- Canonical public probe: `404`
- Legacy public probe: `204`

This proved the live relay at `https://qsl.ddnsfree.com` was stale and still serving only the legacy path-token pull shape.

## Operator actions taken
1. Built the current qsl-server release binary from `main` with `cargo build --release --locked`.
2. Staged the built artifact and checksum locally as `/tmp/qsl-server-linux-x86_64` and `/tmp/qsl-server-linux-x86_64.sha256`.
3. Synced the updated `scripts/` tree to the sanctioned host at `/tmp/directive147-qsl-server-scripts/`.
4. Ran the repo update path on the host:
   - `sudo bash /tmp/directive147-qsl-server-scripts/aws_update_and_verify.sh --artifact-url file:///tmp/qsl-server-linux-x86_64 --checksum-url file:///tmp/qsl-server-linux-x86_64.sha256`
5. After the first live verify exposed a missing `/var/log/qsl-server` bootstrap in `verify_remote.sh`, fixed the script, re-synced `scripts/`, and reran:
   - `sudo BASE_URL=http://127.0.0.1:8080 PUBLIC_BASE_URL=https://qsl.ddnsfree.com /tmp/directive147-qsl-server-scripts/verify_remote.sh`

## After restore: live compatibility proof
- Canonical loopback probe: `204`
- Legacy loopback probe: `204`
- Canonical public probe: `204`
- Legacy public probe: `204`

The restore brought the deployed relay back to the current canonical header-based API while preserving legacy compatibility truthfully.

## qsc real-relay proof
A fresh headless `qsc` relay-test flow against `https://qsl.ddnsfree.com` produced the expected markers:

```text
QSC_TUI_RELAY_TEST result=started code=pending
QSC_TUI_RELAY_TEST result=ok code=relay_authenticated
QSC_MARK/1 event=tui_relay_test_done ok=true reason=relay_authenticated
```

This proved the current `qsc` canonical relay-test path now authenticates successfully against the restored live relay.

## Guard hard-coding proof
- `scripts/check_relay_compatibility.sh` now acts as the single compatibility probe for loopback and public bases.
- The guard fails with `QSL_RELAY_COMPAT_RESULT FAIL code=legacy_only_deploy` when canonical `/v1/pull?max=1` is unavailable but legacy `/v1/pull/:channel?max=1` still answers.
- `scripts/verify_remote.sh` now:
  - bootstraps `/var/log/qsl-server` before logging,
  - runs canonical compatibility preflight before push/pull sanity,
  - derives the public HTTPS base from Caddy when possible.
- `scripts/aws_update_and_verify.sh` now runs the same compatibility preflight after update/restart.
- `scripts/ci/test_relay_deploy_compatibility_guard.sh` now spins a mocked legacy-only endpoint and asserts the exact `legacy_only_deploy` failure code, so future stale deployments fail fast in project validation.

## Evidence hygiene
- No bearer tokens, route tokens, or capability-like secrets were printed in docs, logs, or this evidence note.
- qsl-server remained transport-only throughout this item.
