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

## Remote Linux deployment proof (systemd + smoke)

### Build release binary
```bash
cargo build --release
```

### Copy to host
```bash
scp target/release/qsl-server user@HOST:/opt/qsl-server/qsl-server
```

### systemd unit (qsl-server.service)
```
[Unit]
Description=QSL Transport-only Relay
After=network.target

[Service]
Type=simple
WorkingDirectory=/opt/qsl-server
ExecStart=/opt/qsl-server/qsl-server
Environment=PORT=8080
Restart=always
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths=/var/log/qsl-server

[Install]
WantedBy=multi-user.target
```

### Firewall notes (example)
```bash
# allow 8080/tcp
sudo ufw allow 8080/tcp
```

### Smoke checks
```bash
# push
curl -X POST --data-binary @/path/to/file.bin http://HOST:8080/v1/push/testchan

# pull (should return bytes)
curl -v http://HOST:8080/v1/pull/testchan --output /tmp/pulled.bin

# second pull should be empty
curl -v http://HOST:8080/v1/pull/testchan
```

## Scope boundary
- Payloads are opaque bytes; the relay does not parse or interpret protocol messages.
