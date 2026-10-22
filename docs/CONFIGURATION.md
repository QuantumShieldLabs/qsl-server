# Relay configuration

Status: Supporting
Owner: QSL maintainers
Last-Updated: 2026-10-22

This guide preserves the configuration values documented on the reviewed public branch. Validate values against the exact server revision you run. The service remains research-stage and is not approved for production deployment.

CLI overrides environment; environment overrides defaults. Invalid numeric values fail startup.

| Setting | Published default | Behavior |
|---|---|---|
| `MAX_BODY_BYTES` | 1 MiB | `413 ERR_TOO_LARGE` on excess |
| `MAX_QUEUE_DEPTH` | 257 | `429 ERR_OVERLOADED` on excess |
| `MAX_ROUTE_COUNT` | 256 | `429 ERR_ROUTE_CAP` on excess |
| `PUSH_RATE_BURST` / `PUSH_RATE_REFILL_PER_SEC` | 257 / 257 | `429 ERR_RATE_LIMITED` |
| `ROUTE_IDLE_TTL_MS` | 300000 | Idle routes reclaimed |
| `PULL_LEASE_SECS` | 60 | Unacknowledged messages become eligible for redelivery |
| `RETENTION_TTL_SECS` | 604800 | Seven-day message retention |
| `MAX_INVITE_SLOTS` | 256 | `429 ERR_INVITE_CAP_FULL`; existing slots are not evicted |
| `INVITE_CREATE_BURST` / `INVITE_CREATE_REFILL_PER_SEC` | 32 / 1 | Global invitation-create rate limit |
| `MAX_INVITE_BUNDLE_BYTES` | 16384 | `413 ERR_INVITE_TOO_LARGE` |
| `MAX_INVITE_EXPIRY_SECS` | 259200 | Requested expiry is clamped |
| `BIND_ADDR` | `127.0.0.1` | Loopback by default; public bind needs explicit opt-in |

`RELAY_TOKEN`, when configured, enables bearer authentication. It is separate from route capabilities. Without it, anyone able to reach the port can attempt requests. Inspect `/v1/server-info` to confirm the actual mode.

## Interface notes

The messaging endpoints are `/v1/push`, `/v1/pull`, `/v1/pull/ack` and `/v1/server-info`. Route authorization uses `X-QSL-Route-Token`. `X-Msg-Id` is not an idempotency key.

Invitation creation, redemption and revocation are JSON POSTs under `/v1/invite/`. The client mints the redemption capability and supplies its digest at creation. Redemption returns the bundle and one-shot ticket. Invite-slot pushes require `X-QSL-Invite-Ticket`. Ordinary routes have different admission behavior.

Keep credentials out of URLs and logs. Record only sanitized synthetic values in reports. A relay-wide bearer token does not hide metadata or prove safe resource isolation for hostile public traffic.
