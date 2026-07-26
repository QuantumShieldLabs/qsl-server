# qsl-server (transport-only relay)

Transport-only relay for QSL demos. It forwards/stores **opaque** payloads and must not interpret protocol messages.

## Public posture and licensing
- Source in this repository is public and licensed under `AGPL-3.0-only`; see `LICENSE`.
- This repository contains the public relay code and operator documentation for the transport-only server boundary.
- Any separate commercial services or support offerings would be distinct from this repository and do not replace the AGPL terms for the source published here.

## Invariants
- No protocol parsing, no crypto, no wire changes.
- Fail-closed with deterministic errors.
- No secret/payload logging.

## API
- Canonical push: `POST /v1/push` with `X-QSL-Route-Token: <token>` -> `{ "id": "<msg_id>" }`
- Canonical pull: `GET /v1/pull?max=N` with `X-QSL-Route-Token: <token>` -> JSON `{ "items": [ { "id": "<msg_id>", "data": [<byte>, ...] }, ... ] }` (200) or 204 if empty
- Optional `X-Msg-Id` supplies an opaque message identifier. It is not an idempotency key: duplicate values are accepted as separate queued messages. Accepted message IDs are logged as non-secret operational metadata, so clients must not put secrets in this header.
- Legacy path-token routes are retired. `POST /v1/push/{channel}` and `GET /v1/pull/{channel}?max=N` are no longer supported because they carry the route token in the request URI.
- Invite slots (NA-0678): `POST /v1/invite/create`, `POST /v1/invite/redeem`, `POST /v1/invite/revoke`. All three are POSTs carrying `invite_id` and any secret in the JSON **body** — never in a path or query parameter, for the same reason the legacy route-token paths above were retired.
  - `create` accepts `{invite_id, cap_hash, expiry, bundle_b64, invite_sig_b64}` and returns `{revoke_token}`. The **client** mints the capability and uploads only its SHA-256; the relay never holds a capability in plaintext before a redeemer presents one, and **there is no mint endpoint**.
  - `redeem` accepts `{invite_id, cap}` and returns `{bundle_b64, invite_sig_b64, ticket}`. Consumption is an atomic compare-and-set: exactly one redemption of a slot can win, and every other gets `ERR_INVITE_ALREADY_USED`.
  - `revoke` accepts `{invite_id, revoke_token}` and is idempotent.
  - The `ticket` is a **one-shot** credential for the handshake push: `POST /v1/push` to an invite slot requires `X-QSL-Invite-Ticket`. Pushes to routes that are not invite slots are unaffected.
  - The relay stores `bundle` and `invite_sig` as **opaque bytes** and never parses them. Consumed and revoked slots are **tombstoned until expiry** (blobs cleared) so that "already used" stays distinguishable from "never existed".

## Behavior and limits
- `MAX_BODY_BYTES` (default 1 MiB) → 413 + `ERR_TOO_LARGE`
- `MAX_QUEUE_DEPTH` (default 257) → 429 + `ERR_OVERLOADED`
- `MAX_ROUTE_COUNT` (default 256) caps live route slots. Accepted pushes to new routes create slots only when the cap allows; new-route pushes beyond the cap return 429 + `ERR_ROUTE_CAP`.
- `PUSH_RATE_BURST` (default 257) and `PUSH_RATE_REFILL_PER_SEC` (default 257, `0` allowed to disable refill) provide a local in-app per-route push token bucket. Pushes beyond available tokens return 429 + `ERR_RATE_LIMITED`.
- `ROUTE_IDLE_TTL_MS` (default 300000, capped at 86400000) applies a Time-based idle TTL to route slots. Cleanup runs deterministically on canonical push/pull after auth, route-token, body-size, and pull-`max` validation. Expired routes are removed with queued messages discarded, releasing route capacity and per-route rate accounting before the current accepted request is evaluated.
- Empty body → 400 + `ERR_EMPTY_BODY`
- Missing limit values use defaults. Non-numeric values fail startup with deterministic config errors. Zero values fail startup for `MAX_BODY_BYTES`, `MAX_QUEUE_DEPTH`, `MAX_ROUTE_COUNT`, `PUSH_RATE_BURST`, and `ROUTE_IDLE_TTL_MS`; `PUSH_RATE_REFILL_PER_SEC=0` is allowed for deterministic no-refill operation. Values above the built-in ceilings are capped.
- `RELAY_TOKEN` is optional. When set, canonical push/pull require `Authorization: Bearer <token>` and reject missing or invalid bearer tokens with 401 `ERR_UNAUTHORIZED` before mutating queues. When unset or empty, relay auth is disabled and route-token header checks still apply.
- Unknown pulls return 204 without creating route slots. Draining a route to empty removes the live slot, releasing global route capacity and per-route rate accounting.
- `MAX_INVITE_SLOTS` (default 256, ceiling 4096) caps live invite slots; beyond it, `create` returns 429 + `ERR_INVITE_CAP_FULL` and **never evicts an existing slot** — an eviction path would let an attacker delete other people's invites.
- `INVITE_CREATE_BURST` (default 32) and `INVITE_CREATE_REFILL_PER_SEC` (default 1, `0` allowed) provide a **global** invite-create token bucket returning 429 + `ERR_RATE_LIMITED`. It is global rather than per-route because an invite has no route token until it exists. The cap and the bucket are both required and are not substitutes: the cap bounds storage, the bucket bounds denial.
- `MAX_INVITE_BUNDLE_BYTES` (default 16384, ceiling 65536) → 413 + `ERR_INVITE_TOO_LARGE`. `MAX_INVITE_EXPIRY_SECS` (default 259200 = 72 h, ceiling 30 days) clamps a requested expiry to what this relay offers.
- Rate and global route-cap controls are minimal local in-app hardening primitives. They do not approve production deployment, and reverse proxy / edge rate limiting remains a separate deployment layer.

## Run (local)
```bash
cargo run
# listens on 127.0.0.1:8080 by default
```

CLI overrides env, env overrides defaults:

```bash
qsl-server --bind 0.0.0.0 --port 8080 --max-body-bytes 1048576 --max-queue-depth 257 --max-route-count 256 --push-rate-burst 257 --push-rate-refill-per-sec 257 --route-idle-ttl-ms 300000
```

Environment defaults:
- `BIND_ADDR=127.0.0.1` (safe default, explicit opt-in needed for public bind)
- `PORT=8080`
- `MAX_BODY_BYTES=1048576`
- `MAX_QUEUE_DEPTH=257`
- `MAX_ROUTE_COUNT=256`
- `PUSH_RATE_BURST=257`
- `PUSH_RATE_REFILL_PER_SEC=257`
- `ROUTE_IDLE_TTL_MS=300000`

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
- canonical relay compatibility on loopback and, when derivable, the public TLS host
- push/pull sanity
- deployed git HEAD

Fail-fast rule:
- Run `scripts/verify_remote.sh` before any real-world validation or qsc relay test.
- A deployment that still answers the legacy path-token pull shape is deployment drift, not a weak-host saturation result and not a qsl-attachments defect.

## Scope boundary
- Payloads are opaque bytes; the relay does not parse or interpret protocol messages.
- Transport-only relay; no protocol or cryptographic behavior is implemented here.
