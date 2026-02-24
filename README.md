# qsl-server (transport-only relay)

Transport-only relay for QSL demos. It forwards/stores **opaque** payloads and must not interpret protocol messages.

## Invariants
- No protocol parsing, no crypto, no wire changes.
- Fail-closed with deterministic errors.
- No secret/payload logging.

## API (v0)
- POST   /v1/push/{channel}         -> { "id": "<msg_id>" }
- GET    /v1/pull/{channel}         -> oldest message bytes (200) or 204 if empty

## Behavior and limits
- `MAX_BODY_BYTES` (default 1 MiB) → 413 + `ERR_TOO_LARGE`
- `MAX_QUEUE_DEPTH` (default 256) → 429 + `ERR_QUEUE_FULL`
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
- push/pull sanity
- deployed git HEAD

## Scope boundary
- Payloads are opaque bytes; the relay does not parse or interpret protocol messages.
- Transport-only relay; no protocol or cryptographic behavior is implemented here.
