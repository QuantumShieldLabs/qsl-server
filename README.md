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
# listens on 0.0.0.0:8080 by default
```

CLI overrides env, env overrides defaults:

```bash
qsl-server --bind 0.0.0.0 --port 8080 --max-body-bytes 1048576 --max-queue-depth 256
```

## Remote deployment (Ubuntu 24.04 + systemd)

The repo includes a reproducible install script and a systemd unit template.

```bash
# copy scripts to the host, then run as root:
sudo bash scripts/install_ubuntu_24_04_systemd.sh
```

Artifacts:
- systemd unit: `systemd/qsl-server.service`
- install script: `scripts/install_ubuntu_24_04_systemd.sh`
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
