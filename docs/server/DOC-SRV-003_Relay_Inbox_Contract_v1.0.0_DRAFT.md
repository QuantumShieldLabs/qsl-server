# DOC-SRV-003 — Relay Inbox Contract (Store-and-Forward) v1.0.0 (DRAFT)

## Purpose
Define a minimal, explicit store-and-forward relay inbox contract for ciphertext-only payloads to enable two-way client exchange without server-side protocol drift.

## Threat model
- Relay is untrusted for payload content: it stores opaque ciphertext blobs only.
- Relay must not log payload contents or secrets.

## API shape (minimal)

### PUSH (store)
- Input:
  - channel_id (opaque, client-chosen)
  - message_id (opaque, client-chosen; idempotency key)
  - ciphertext blob
- Behavior:
  - Accepts only opaque ciphertext.
  - Enforces hard limits (see Limits).
  - Deterministic reject on oversize or overflow.

### PULL (fetch)
- Input:
  - channel_id (opaque)
  - max_items (bounded)
- Output:
  - list of ciphertext blobs (bounded)
- Delivery semantics:
  - Delete-on-deliver (preferred) OR explicit ack (if delete-on-deliver cannot be guaranteed).
  - Choice must be explicit and documented; default is delete-on-deliver.

## Limits (hard, deterministic)
- MAX_BODY_BYTES: server-enforced ceiling per message (document current value; recommend 1 MiB unless otherwise configured).
- MAX_QUEUE_DEPTH: bounded queue per channel (document current value; recommend 256 unless otherwise configured).
- Overflow behavior:
  - Reject new PUSH deterministically when queue is full.

## Retention / TTL
- Retention is bounded (time-based TTL or size-based eviction).
- Default behavior is deterministic and documented (e.g., TTL=24h).

## Authentication (explicit)
- Optional bearer token auth (if enabled). If disabled, document that access is protected by network controls.
- Auth mode must be explicit and stable per deployment.

## Logging policy
- No payload logging (ciphertext blobs never logged).
- Log only minimal metadata: channel_id hash/prefix and message_id hash/prefix if needed for ops.

## Determinism
- Rejects are deterministic and stable (same inputs → same error codes).
- Client-side markers should reflect PUSH/PULL outcomes deterministically.

## Client integration notes
- `qsc receive` is explicit-only (no background polling).
- Client markers should include:
  - recv_start / recv_item / recv_commit / recv_none / recv_error
- Client must not emit secrets in logs/markers.

## Implementation (current canonical)
- Canonical endpoint: `POST /v1/push`
  - Required header: `x-qsl-route-token`
  - Optional header: `x-msg-id` (client-provided opaque id; server will generate if absent).
  - Body: raw ciphertext blob (opaque).
  - Rejects:
    - missing/empty route-token header → 400 `ERR_MISSING_ROUTE_TOKEN`
    - oversize → 413 `ERR_TOO_LARGE`
    - queue full → 429 `ERR_QUEUE_FULL`
- Canonical endpoint: `GET /v1/pull?max=N`
  - Required header: `x-qsl-route-token`
  - Returns 204 if empty.
  - Returns JSON `{ "items": [ { "id": "<opaque>", "data": [u8...] }, ... ] }`.
- Legacy `/v1/push/:channel` and `/v1/pull/:channel?max=N` are retired and must not be reintroduced because they carry route tokens in the request URI.
- Retention/TTL: not implemented yet; bounded by queue depth only (follow-on).

## Deployment notes
- TLS termination must be explicit (typically at ALB/Nginx/Caddy).
- This contract is docs-only; no behavior changes are implied until implementation PRs.
