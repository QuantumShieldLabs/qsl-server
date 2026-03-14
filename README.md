# qsl-server (transport-only relay)

Transport-only relay for QSL demos. It forwards/stores **opaque** payloads and must not interpret protocol messages.

## Invariants
- No protocol parsing, no crypto, no wire changes.
- Fail-closed with deterministic errors.
- No secret/payload logging.

## API (canonical + compatibility window)
- Canonical push: `POST /v1/push` with `X-QSL-Route-Token: <token>` -> `{ "id": "<msg_id>" }`
- Canonical pull: `GET /v1/pull?max=N` with `X-QSL-Route-Token: <token>` -> oldest message bytes (200) or 204 if empty
- Compatibility-only legacy shape: `POST /v1/push/{channel}` and `GET /v1/pull/{channel}?max=N`
- Compatibility note: `{channel}` on the legacy shape carries the route token in the request URI. Decision D-0009 makes the header-based shape canonical and keeps the path-based shape only for the compatibility window; do not place real route tokens in docs, screenshots, or copied command lines.

## Behavior and limits
- `MAX_BODY_BYTES` (default 1 MiB) → 413 + `ERR_TOO_LARGE`
- `MAX_QUEUE_DEPTH` (default 256) → 429 + `ERR_OVERLOADED`
- Empty body → 400 + `ERR_EMPTY_BODY`

## Run (local)
```bash
cargo run
# listens on 127.0.0.1:8080 by default
```

CLI overrides env, env overrides defaults:

```bash
qsl-server --bind 0.0.0.0 --port 8080 --max-body-bytes 1048576 --max-queue-depth 256
```

Environment defaults:
- `BIND_ADDR=127.0.0.1` (safe default, explicit opt-in needed for public bind)
- `PORT=8080`

## Remote deployment (Ubuntu 24.04 + systemd)

The repo includes reproducible install/update scripts and packaging templates.

```bash
# copy scripts to the host, then run as root:
sudo bash scripts/install_ubuntu.sh /path/to/qsl-server
# later updates:
sudo bash scripts/update_ubuntu.sh /path/to/qsl-server
```

Artifacts:
- systemd unit: `packaging/systemd/qsl-server.service`
- env template: `packaging/systemd/relay.env.example`
- caddy example: `packaging/caddy/Caddyfile.example`
- production runbook: `packaging/runbook_ubuntu.md`
- install script: `scripts/install_ubuntu.sh`
- update script: `scripts/update_ubuntu.sh`
- checksum-verified release update: `scripts/update_from_release.sh --release vX.Y.Z`
- audit script: `scripts/qsl_relay_audit.sh`
- verify script: `scripts/verify_remote.sh`

### Firewall notes (example)
```bash
# allow 8080/tcp
sudo ufw allow 8080/tcp
```

## Verify deployment (on the host)
```bash
sudo bash scripts/verify_remote.sh
```

The verify script checks:
- systemd active status
- listener on port 8080
- deployed binary metadata from `/opt/qsl-server/DEPLOYMENT_INFO`
- installed binary checksum
- push/pull sanity
- deployed git HEAD

## Scope boundary
- Payloads are opaque bytes; the relay does not parse or interpret protocol messages.
- Transport-only relay; no protocol or cryptographic behavior is implemented here.
