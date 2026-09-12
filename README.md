[README.md](https://github.com/user-attachments/files/32137390/README_qsl-server.md)

# QSL Server

**The QSL relay: a mailbox for sealed bytes.**

Clients push encrypted frames to a mailbox and pull them back out. The relay stores and
forwards. It holds no session keys, performs no ratchet operations, and cannot read message
content.

> [!WARNING]
> **Research-stage. Not independently audited. Not production-ready.**
> This is demo and interop infrastructure. Rate limiting, route caps, and idle expiry here are
> minimal **local in-app** hardening primitives, not a substitute for an edge layer.

---

## What the relay does and does not see

This deserves precision, because "the server can't see anything" is a claim people make too
loosely.

**On the messaging plane, the relay is opaque transport.** It accepts sealed frames, holds
them in a mailbox addressed by a route token, and hands them back. It never parses a protocol
message, performs no cryptography on payloads, and cannot decrypt anything.

**The invite plane is deliberately server-mediated.** Establishing a new contact uses relay
endpoints that create, redeem, and revoke invitation slots. The relay compares a presented
capability against a stored SHA-256 digest and burns one-shot tickets. That is what makes an
invitation genuinely single-use and revocable, and it is protocol participation — we name it
rather than describe the relay as semantics-free and let a reviewer find the invite routes.
Even there, the invitation `bundle` and `invite_sig` are stored as opaque bytes and are never
parsed.

**What a relay operator can still observe.** Traffic metadata: which mailboxes are active, how
much, how large, and when. Message content is end-to-end encrypted and unreadable to the
relay. **Metadata resistance is open work, not a solved problem**, and running your own relay
is the current answer.

---

## Design invariants

- No protocol message parsing. No payload cryptography. No wire-format decisions.
- Fail-closed with deterministic, documented error codes.
- No secret or payload logging.
- Route tokens are stored only as digests.
- Secrets travel in request **bodies**, never in a path or query parameter. Legacy path-token
  routes were retired for exactly this reason: a URI ends up in logs, proxies, and history.

---

## API

### Messaging plane

| Route | Notes |
|---|---|
| `POST /v1/push` | Header `X-QSL-Route-Token`. Returns `{"id": "<msg_id>"}`. |
| `GET /v1/pull?max=N&ack=lease` | Header `X-QSL-Route-Token`. Returns `{"items":[{"id","data"}]}` or `204` when empty. |
| `POST /v1/pull/ack` | Retires leased messages by id. |
| `GET /v1/server-info` | Reports configuration and auth mode. |

`X-Msg-Id` optionally supplies an opaque message identifier. **It is not an idempotency key** —
duplicate values are accepted as separate queued messages. Accepted ids are logged as
non-secret operational metadata, so never put a secret in that header.

### Invite plane

All three are POSTs carrying `invite_id` and any secret in the JSON body.

- **`POST /v1/invite/create`** — `{invite_id, cap_hash, expiry, bundle_b64, invite_sig_b64}`
  returns `{revoke_token}`. The **client** mints the capability and uploads only its SHA-256.
  The relay never holds a capability in plaintext before a redeemer presents one, and **there
  is no mint endpoint**.
- **`POST /v1/invite/redeem`** — `{invite_id, cap}` returns `{bundle_b64, invite_sig_b64,
  ticket}`. Consumption is an atomic compare-and-set: exactly one redemption wins and every
  other gets `ERR_INVITE_ALREADY_USED`.
- **`POST /v1/invite/revoke`** — `{invite_id, revoke_token}`, idempotent.

The returned `ticket` is a **one-shot** credential for the handshake push: a `POST /v1/push`
to an invite slot requires `X-QSL-Invite-Ticket`. Pushes to ordinary routes are unaffected.

Consumed and revoked slots are **tombstoned until expiry** with their blobs cleared, so
"already used" stays distinguishable from "never existed".

---

## Configuration

Every limit has a default, a ceiling, and a deterministic error. Non-numeric values fail
startup rather than falling back silently. CLI overrides env; env overrides defaults.

| Setting | Default | On breach |
|---|---|---|
| `MAX_BODY_BYTES` | 1 MiB | `413 ERR_TOO_LARGE` |
| `MAX_QUEUE_DEPTH` | 257 | `429 ERR_OVERLOADED` |
| `MAX_ROUTE_COUNT` | 256 | `429 ERR_ROUTE_CAP` |
| `PUSH_RATE_BURST` / `PUSH_RATE_REFILL_PER_SEC` | 257 / 257 | `429 ERR_RATE_LIMITED` |
| `ROUTE_IDLE_TTL_MS` | 300000 | idle routes reclaimed |
| `PULL_LEASE_SECS` | 60 | unacked messages redelivered |
| `RETENTION_TTL_SECS` | 604800 (7 days) | expired messages swept |
| `MAX_INVITE_SLOTS` | 256 | `429 ERR_INVITE_CAP_FULL` |
| `INVITE_CREATE_BURST` / `INVITE_CREATE_REFILL_PER_SEC` | 32 / 1 | `429 ERR_RATE_LIMITED` |
| `MAX_INVITE_BUNDLE_BYTES` | 16384 | `413 ERR_INVITE_TOO_LARGE` |
| `MAX_INVITE_EXPIRY_SECS` | 259200 (72 h) | requested expiry clamped |
| `BIND_ADDR` | `127.0.0.1` | public bind requires explicit opt-in |

**Two deliberate choices worth explaining:**

The invite slot cap **never evicts an existing slot** when full. An eviction path would let an
attacker delete other people's invitations by flooding.

The invite-create bucket is **global** rather than per-route, because an invitation has no
route token until it exists. The cap and the bucket are both required and are not substitutes:
the cap bounds storage, the bucket bounds denial.

### Authentication

`RELAY_TOKEN` is optional. When set, push and pull require `Authorization: Bearer <token>` and
reject a missing or invalid token with `401 ERR_UNAUTHORIZED` before any queue mutation. When
unset, relay bearer auth is disabled and only route-token header checks apply.

> [!IMPORTANT]
> A relay with no `RELAY_TOKEN` configured is **open to anyone who can reach the port**.
> `/v1/server-info` reports which mode is in force. Check it after deploying.

---

## Run

```bash
cargo run
# listens on 127.0.0.1:8080 by default
```

```bash
qsl-server --bind 0.0.0.0 --port 8080 \
  --max-body-bytes 1048576 --max-queue-depth 257 --max-route-count 256 \
  --push-rate-burst 257 --push-rate-refill-per-sec 257 --route-idle-ttl-ms 300000
```

---

## Status and honest limits

- **Not independently audited.** No third party has reviewed this relay.
- **Not production-ready.** The local in-app hardening primitives here bound local abuse; they do not
  approve a production deployment. Reverse-proxy and edge rate limiting remain a separate
  layer you must supply.
- **Metadata is visible to the operator.** See above. Run your own.
- Open defects exist and are tracked in the open.

---

## Security reporting

Please do **not** file security-sensitive reports in public issues. Use GitHub private
vulnerability reporting on this repository, or follow [`SECURITY.md`](SECURITY.md). If private
reporting is unavailable, open a minimal public issue with **no exploit details**, stating
that you can share specifics privately.

## Related repositories

- [**qsl-protocol**](https://github.com/QuantumShieldLabs/qsl-protocol) — specifications,
  conformance vectors, and the reference implementation
- [**qsl-desktop**](https://github.com/QuantumShieldLabs/qsl-desktop) — the desktop client
- [**qsl-attachments**](https://github.com/QuantumShieldLabs/qsl-attachments) — encrypted
  attachment plane, a separate service surface

## License

`AGPL-3.0-only` — see [`LICENSE`](LICENSE). Any future commercial services or support
offerings are separate from this repository and do not replace the AGPL terms on the source
published here.
