# DOC-SRV-007 Invite Slot Contract v1.0.0 (DRAFT)

## Purpose

Define the invite-slot routes (NA-0678, directive D614; messaging epic Slice 1):
the relay-side primitives by which one party publishes a single-use invite and
another redeems it to reach them. The routes are ADDITIVE: `/v1/push`,
`/v1/pull`, `/v1/pull/ack` and `/v1/server-info` semantics are unchanged for
every route the invite system did not create.

## Threat model and what the relay is trusted with

The relay is **not** trusted with identity. It stores the published identity
bundle and its signature as **opaque bytes** and never parses, validates or
branches on their content; the cryptographic meaning lives entirely in the
client. What the relay learns is that an invite exists and when it was redeemed.

Two consequences are load-bearing:

1. **The client mints the redemption capability.** It uploads only `SHA-256(cap)`.
   No relay-side path holds a capability in plaintext before a redeemer presents
   one, so a relay operator cannot silently consume — "burn" — an invite it
   hosts. **There is deliberately no mint endpoint.**
2. **Neither `invite_id` nor any secret travels in a URI.** All three routes are
   POSTs carrying their values in the JSON body. `invite_id` *is* the mailbox
   route key, and D-0008/D-0009/D-0010 already retired URI-carried route tokens
   because they leak through proxy logs, shell history and traces.

At rest, `invite_id` is stored only as its SHA-256 digest (via the same
`route_key_for` used for route tokens), so a stolen store file yields no usable
mailbox keys. `cap`, `revoke_token` and `ticket` are likewise stored only as
digests and compared in constant time with the same `ct_eq_secret` used for the
bearer token (D-0014) — no new primitive is introduced.

## Routes (normative)

### `POST /v1/invite/create`
Body `{invite_id, cap_hash, expiry, bundle_b64, invite_sig_b64}` →
`200 {revoke_token}`.

`cap_hash` is the client's `SHA-256(cap)`, hex. `bundle_b64` / `invite_sig_b64`
are base64url. `expiry` is unix seconds and is **clamped** to
`MAX_INVITE_EXPIRY_SECS` rather than rejected: an over-long request is the client
asking for more than this relay offers, and the relay's ceiling governs.

`revoke_token` is a 128-bit CSPRNG value returned **exactly once** and stored
only as a digest. Without it, revoke would be unauthorized on an open relay —
anyone who had merely *seen* an invite code could destroy it.

Rejects: `ERR_INVITE_BAD_BODY` (400) · `ERR_INVITE_TOO_LARGE` (413) ·
`ERR_INVITE_EXPIRED` (400, expiry already past) · `ERR_INVITE_DUPLICATE` (409) ·
`ERR_INVITE_CAP_FULL` (429) · `ERR_RATE_LIMITED` (429) · `ERR_UNAUTHORIZED` (401).

### `POST /v1/invite/redeem`
Body `{invite_id, cap}` → `200 {bundle_b64, invite_sig_b64, ticket}`.

Consumption is an **atomic compare-and-set**: the update re-asserts the ACTIVE
state, so a lost race updates zero rows and returns `ERR_INVITE_ALREADY_USED`.
Exactly one redemption of a slot can win.

`ticket` is a 128-bit **one-shot** credential for the handshake push (below).

Cause order is deliberate: not-found → revoked → expired → already-used →
cap-invalid. Reaching this route requires knowing `invite_id`, a 128-bit secret
carried only inside the invite code, so a caller who can address the slot already
holds the capability; reporting the slot's true state to them discloses nothing
they were not given, and the failure taxonomy requires those causes to stay
distinct.

Rejects: `ERR_INVITE_NOT_FOUND` (404) · `ERR_INVITE_REVOKED` (410) ·
`ERR_INVITE_EXPIRED` (410) · `ERR_INVITE_ALREADY_USED` (409) ·
`ERR_INVITE_CAP_INVALID` (403).

### `POST /v1/invite/revoke`
Body `{invite_id, revoke_token}` → `200 {revoked: true}`. Idempotent: a second
revoke succeeds. The credential is checked **before** any state is reported,
because unlike redemption a revoke needs a secret the invite code does not carry.

Rejects: `ERR_INVITE_NOT_FOUND` (404) · `ERR_INVITE_REVOKE_INVALID` (403).

### Handshake ingress — `POST /v1/push` to an invite slot
A push whose route token resolves to a known invite slot is admitted **only**
when it presents a live `X-QSL-Invite-Ticket`. The ticket is issued by the
redemption that consumed the slot and is burned on first use inside the same
transaction as the message insert, so the slot accepts **exactly one** handshake
— from the party that actually redeemed it, not merely from anyone who saw the
code and lost the race.

The ticket is a header rather than a body field because `/v1/push`'s body is the
opaque handshake payload and cannot be repurposed. This matches the
`X-QSL-Route-Token` precedent.

Rejects: `ERR_INVITE_TICKET_INVALID` (403) · `ERR_INVITE_EXPIRED` (410) ·
`ERR_INVITE_REVOKED` (410).

**Pull is deliberately ungated**: the slot's creator addresses it with the same
`invite_id` and must be able to collect the handshake.

**Pushes to routes that are not invite slots are entirely unaffected** — one
indexed lookup misses and the pre-existing path runs unchanged. This is the
compatibility guarantee that lets the existing client keep working until the
client-side slices land.

## Tombstoning (normative, not an optimisation)

Consumed and revoked slots **persist until `expiry`** with their `bundle` and
`invite_sig` blobs cleared; only state and timestamps survive, so a tombstone
carries no identity material. Deleting them instead would collapse
`invite-already-used` into `invite-not-found` and tell a redeemer "never existed"
when the truth is "someone got here first" — which is precisely the interception
signal the invite design exists to surface. The slot's own expiry bounds the
tombstone's cost.

## Limits

| knob | default | ceiling |
|---|---|---|
| `MAX_INVITE_SLOTS` | 256 | 4096 |
| `MAX_INVITE_BUNDLE_BYTES` | 16384 | 65536 |
| `MAX_INVITE_EXPIRY_SECS` | 259200 (72 h) | 2592000 (30 days) |
| `INVITE_CREATE_BURST` | 32 | 4096 |
| `INVITE_CREATE_REFILL_PER_SEC` | 1 | 4096 (`0` allowed) |

**The slot cap and the create-rate bucket are both required and are not
substitutes.** The cap bounds storage; the bucket bounds denial. The bucket is
**global**, not per-route, because invite creation has no route token yet — the
per-route push bucket structurally cannot cover it. On an open relay a cap alone
would let any anonymous caller fill every slot in one burst and deny invite
creation to everyone until those slots expired.

`ERR_INVITE_CAP_FULL` **never evicts**. An eviction path would let an attacker
delete other people's invites, which is a worse failure than the denial it would
relieve.

## Durability

An accepted `create` is **fsynced before its 200 reaches the socket**. This is
proven by `tests/na0678_invite_durability.rs`, which counts real
`fsync`/`fdatasync` syscalls and asserts the ordering, and which **skips with a
stated reason** when `strace` is unavailable rather than passing silently.

⚠ A restart test is **not** evidence for this property: `SIGKILL` destroys a
process, not the OS page cache, so `synchronous=FULL` and `synchronous=OFF` are
indistinguishable to it. See the corrected header of
`tests/na0642_durability_restart.rs`.

## Capability advertisement

`GET /v1/server-info` gains `api: [… , "invite_v1"]`,
`limits.max_invite_bundle_bytes`, and an `invite` object carrying
`{max_expiry_secs, max_slots}` — additive per DOC-SRV-006 rule 1: nothing
removed, renamed or repurposed.

## Storage

`SCHEMA_VERSION` advances to 2 for the `invites` table. The migration now
**advances the stored marker**, which it previously did not: the value was
written with `INSERT OR IGNORE`, a no-op on an existing key, so the D-0011
downgrade guard went inert after any schema change. See DECISIONS `D-0016`.

## Decision

Recorded as qsl-server DECISIONS `D-0016` (wire-contract surface, following
`D-0009`/`D-0010`/`D-0011`/`D-0012`). Governance authority: qsl-protocol lane
NA-0678, directive QSL-DIR-2026-07-26-614.
