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
  - message_id / `x-msg-id` (opaque, client-chosen identifier when supplied)
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
- MAX_ROUTE_COUNT: bounded live route slots. Accepted pushes to new routes create slots only when the cap allows; unknown pulls do not create slots.
- PUSH_RATE_BURST / PUSH_RATE_REFILL_PER_SEC: local in-app per-route push token bucket. A refill of 0 is allowed for deterministic no-refill operation.
- ROUTE_IDLE_TTL_MS: Time-based idle TTL for live route slots. Expired routes are removed on canonical push/pull after request validation, queued messages are discarded, and route capacity plus per-route rate accounting are released.
- Overflow behavior:
  - Reject new PUSH deterministically when queue is full.
  - Reject new-route PUSH deterministically with `ERR_ROUTE_CAP` when the global live route cap is full.
  - Reject PUSH deterministically with `ERR_RATE_LIMITED` when the route's local token bucket has no available tokens.

## Retention / TTL
- Retention is bounded by delete-on-empty and Time-based idle TTL.
- Current default behavior is deterministic delete-on-empty plus idle-route expiry: pull delivery removes queued messages, draining a route to empty removes the live route slot, and routes idle beyond `ROUTE_IDLE_TTL_MS` are removed on canonical push/pull access. TTL applies to non-empty and empty route state; expired queued messages are not delivered after cleanup.

## Authentication (explicit)
- Optional bearer token auth (if enabled). If disabled, document that access is protected by network controls.
- Auth mode must be explicit and stable per deployment.

## Logging policy
- No payload logging (ciphertext blobs never logged).
- Log only minimal metadata: channel_id hash/prefix and message_id hash/prefix if needed for ops.
- Current implementation logs accepted message IDs as non-secret metadata on push/pull. Clients must not place route tokens, auth material, payload contents, or other secrets in `x-msg-id`.

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
    - rate limited → 429 `ERR_RATE_LIMITED`
    - global route cap full for a new route → 429 `ERR_ROUTE_CAP`
    - queue full → 429 `ERR_OVERLOADED`
- Canonical endpoint: `GET /v1/pull?max=N`
  - Required header: `x-qsl-route-token`
  - Returns 204 if empty.
  - Returns JSON `{ "items": [ { "id": "<opaque>", "data": [u8...] }, ... ] }`.
- Legacy `/v1/push/:channel` and `/v1/pull/:channel?max=N` are retired and must not be reintroduced because they carry route tokens in the request URI.
- Current `x-msg-id` behavior is not idempotency: each accepted push appends a queue item, even when the same identifier is supplied more than once. Idempotent duplicate handling is a future service semantic decision and requires executable tests before it can be claimed.
- Current `x-msg-id` logging boundary: accepted message IDs may appear in service logs as non-secret operational metadata; route tokens, auth headers, and payload bytes must not.
- Current limit config behavior defaults only when values are missing, fails startup for non-numeric values, rejects zero for body, queue-depth, route-count, burst, and route-idle-TTL limits, allows `PUSH_RATE_REFILL_PER_SEC=0`, and caps values above built-in ceilings.
- Local in-app rate limiting and global route-count caps are implemented as bounded memory-only controls. They do not replace reverse proxy, firewall, edge rate limiting, or deployment-layer operational controls.
- Retention/TTL: current route-slot lifecycle is delete-on-empty after pull drain plus Time-based idle TTL cleanup on canonical push/pull access. Cleanup logs only redacted route identifiers and bounded counts; route tokens, auth headers, and payload bytes must not appear.

## Deployment notes
- TLS termination must be explicit (typically at ALB/Nginx/Caddy).
- This contract is docs-only; no behavior changes are implied until implementation PRs.
