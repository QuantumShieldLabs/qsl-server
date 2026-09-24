# DOC-SRV-008 Relay v2 Contract v1.0.0 (DRAFT)

## Purpose

State the v2 relay's contract against the v1 invite-slot contract, `DOC-SRV-007`: the four deltas, by that document's
sections, and where the full v2 design lives.

## Status

DRAFT. Nothing in this document is implemented: no `/v2/` route, table, header or code exists in this repository at
the time of writing (main `5ea0f925`). The relay's v1 contract, `DOC-SRV-007` and the documents it builds on, is
unchanged and remains normative for every `/v1/` route. This document becomes normative for v2 only when the
implementing lanes (PLAN cards F08 and F09) land it with their own review.

## Authority

The design authority is the qsl-protocol contract C03, relay authority and recovery:
`docs/ops/contracts/C03_relay_authority_and_recovery.md` in the qsl-protocol repository (ACCEPTED WITH NAMED FIXES,
recorded there as `D-1428`; governance lane NA-0783, PLAN card F01). Section and row names below (T1-T11, EP1-EP10,
N1-N5, V01-V66) are that contract's. Where this document and C03 disagree, C03 governs. Identifiers are allocated in
qsl-protocol `DOC-CAN-003` section 12.4; the relay derivation label spellings stay PROPOSED there until the Director
rules (C03 open cell C3-O1).

## Scope

This document states ONLY the four deltas between the v2 relay and `DOC-SRV-007` (the invite-slot contract), by that
document's sections. It does not restate the v2 surface: the capability split (R reads, D = H(label ++ R) deposits,
K = H(label ++ D) keys the store), the v2 namespaces, the endpoints and their order tables, request identity, quotas
and cleanup, and the schema migration are C03 T1-T8. The v2 mailbox endpoints (`/v2/mailbox/open`, `/v2/push`,
`/v2/pull`, `/v2/pull/ack`) and the `server-info` additions (`api` gains `relay_v2`; a `v2` object, additive per
DOC-SRV-006 rule 1) are C03 T3.

Kept from `DOC-SRV-007` without change in v2: the client mints the redemption capability and uploads only its hash,
and there is no mint endpoint (`DOC-SRV-007` "Threat model", items 1-2); secrets never travel in a URI; the relay
stores capabilities only as digests and compares them in constant time; the slot cap and a GLOBAL create-rate bucket
are both required ("Limits"); a create is fsynced before its 200 ("Durability").

## The four deltas against DOC-SRV-007, by section

| # | DOC-SRV-007 section | v1 (DOC-SRV-007, unchanged for `/v1/`) | v2 (C03) |
|---|---|---|---|
| 1 | Routes: `POST /v1/invite/create`, expiry | `expiry` is CLAMPED to `MAX_INVITE_EXPIRY_SECS` rather than rejected | `POST /v2/invite/create` REFUSES an expiry outside (now, now + max_expiry]: 400 `ERR_V2_EXPIRY_RANGE` (NEW); no clamp (C03 T3 EP5, T6 Create, vector V43) |
| 2 | Routes: `POST /v1/invite/create` and `POST /v1/invite/revoke`, the revoke credential | the relay mints a 128-bit `revoke_token`, returned exactly once and stored as a digest; revoke presents it | NO `revoke_token`. The inviter creates the slot with its own read secret R_slot (the slot is a v2 mailbox of kind `invite_slot`), and `POST /v2/invite/revoke` is authorized by R_slot: K is derived from the presented R and compared with the slot's K_slot in constant time BEFORE any state is reported; idempotent; reports whether an A1 was accepted before the revoke (C03 T1 row 5, T3 EP9, T6 Revoke) |
| 3 | Routes: `POST /v1/invite/redeem` and "Handshake ingress -- `POST /v1/push` to an invite slot", the ticket | the relay MINTS a one-shot ticket at redeem and BURNS it on the first admitted push; the redeem is consume-and-erase; pull of the slot is deliberately ungated (any code holder can pull) | the redeemer MINTS the ticket T and the recovery secret S in its own local commit; the FIRST CLAIM (`POST /v2/invite/redeem`) is one store transaction that records the request digest, H(S), H(T) and the replayable result; the A1 is deposited by `POST /v2/invite/deliver`, addressed by the slot locator L and gated by the claim's op_id and T; the ticket is valid for exactly ONE envelope digest per claim, and an exact retry returns the saved receipt without a second enqueue, even after the inviter ACKed; a push by the slot's own deposit capability runs the same admission (`slot_admit`); the slot is READ only with R_slot (C03 T1 rows 5-9, T5, T6 Redeem and Deliver, T2 N5; vectors V20-V46). After expiry and the sweep a v2 slot is unknown (404), never an ordinary mailbox (C03 T2 N5, V46) |
| 4 | "Tombstoning (normative, not an optimisation)" | consumed and revoked slots persist until expiry with `bundle` and `invite_sig` CLEARED at consumption | the claim KEEPS the result bytes (the public `bundle` and `invite_sig`) until the redeemer settles (`POST /v2/invite/settle`) or `recovery_until` passes, so a lost redeem response can be recovered by an exact retry; the slot and its claim tombstone live until max(expiry, recovery_until). Privacy trade-off, stated: the inviter's public keys and invite signature stay on the relay, linked to L, for up to the recovery horizon after the claim instead of zero (C03 T5, T7 "PRIVACY TRADE-OFF") |

## Codes

Relay codes C03 marks NEW (`ERR_V2_*`) and the two configuration refusals (`ERR_INVALID_CONFIG_V2_MAX_BODY_BYTES`,
`ERR_INVALID_CONFIG_V2_RETENTION`) are PROPOSED spellings; they are registered here by the implementing lanes (C03
open cell C3-O10). `ERR_INVITE_*` codes are reused on v2 with the meanings C03 T3 and T6 give them.

## Open

C3-O14 (whether these changes to the relay's own invite-slot contract need a server-side ruling beyond THE PLAN) is
the Director's and stays OPEN. The v2 values C03 leaves open (C3-O2 the recovery horizon; C3-O3 the bucket, ring, idle
and ceiling values) are not fixed here.

## v1 note

A v1 finding against `DOC-SRV-007` "Handshake ingress" is recorded in the qsl-protocol improvement ledger as
`ENG-0356`: on `/v1/push` the promised `ERR_INVITE_EXPIRED` (410) is unreachable, because the sweep removes an expired
slot's row with the same timestamp the admission then checks, so the push is admitted as an ordinary route. v1 is not
changed by this document; the repair needs its own lane. Delta 3's v2 rule (a slot never degrades into an ungated
mailbox) is the successor's closure.
