# DOC-SRV-006 Server Info Capability Contract v1.0.0 (DRAFT)

## Purpose

Define the `GET /v1/server-info` capability document (NA-0652, directive D588;
DOC-PROG-004 step 2, locked decision L4): the mechanism by which a client asks
a relay what it offers. The route is ADDITIVE: `/v1/push`, `/v1/pull`, and
`/v1/pull/ack` semantics are unchanged by this contract.

## Contract Rules (normative)

1. **Additive-only evolution.** Fields are never removed, renamed, or
   repurposed. New capabilities arrive as new fields or new values. Clients
   MUST ignore unknown fields. The anticipated per-client admission work
   (qsl-protocol ENG-0036) would later extend `auth.mode` with a new value
   additively; nothing in this contract implements it.
2. **Features, never security.** The document advertises features and limits.
   Nothing in it may relax, disable, or substitute for a security behavior.
   Absence of a field means "not offered", never "not enforced". A client
   MUST NOT weaken any security posture based on document contents.
   `min_client_version` is ADVISORY data for clients; the server does not
   enforce it.
3. **Served behind the same gate.** The full document sits behind the same
   bearer gate (`RELAY_TOKEN`) as the relay routes. On a bearer relay an
   unauthorized request receives the fixed probe (below); the probe registry
   is FIXED at implementation name + auth mode and may never grow operator
   config. The probe is a deliberate minimal disclosure (locked decision L4):
   an unauthenticated probe of `/v1/push` already returns `ERR_UNAUTHORIZED`
   today; the marginal disclosure is the implementation name.
4. **Probe-vs-full semantics.** The three existing routes keep their existing
   auth behavior byte-identical (`401` with plain `ERR_UNAUTHORIZED`); they do
   NOT adopt the probe body. Only `/v1/server-info` serves JSON on the
   unauthorized path.

## Route

- `GET /v1/server-info`. No query parameters are defined. Responses are
  `application/json`.

## Probe (bearer relay, unauthorized)

Returned with HTTP `401` for a missing OR wrong token — identical both ways,
so the response is never a token oracle:

```json
{"server":"qsl-server","auth":{"mode":"bearer"}}
```

This is the complete registry. It identifies "a QSL relay that requires auth"
(distinguishable from not-a-relay) and nothing else.

## Full Document (open relay: any request; bearer relay: valid token)

Returned with HTTP `200`. Every value reflects LIVE configuration, never a
constant. Canonical shape and value sources:

| Field | Source |
|---|---|
| `server` | `"qsl-server"` (implementation name) |
| `version` | `CARGO_PKG_VERSION` |
| `name` | `RELAY_NAME` env var; `""` when unset (empty-safe) |
| `api` | `["push_v1", "pull_v1", "pull_ack_lease_v1"]` (the served API set) |
| `auth.mode` | `"bearer"` iff `RELAY_TOKEN` set non-empty, else `"open"` |
| `limits.max_body_bytes` | live `MAX_BODY_BYTES` config |
| `limits.max_queue_depth` | live `MAX_QUEUE_DEPTH` config |
| `retention.ttl_secs` | live `RETENTION_TTL_SECS` config (validated value the store enforces) |
| `directory.mode` | `"none"` (no directory service offered) |
| `attachments.service_url` | `RELAY_ATTACHMENTS_SERVICE_URL` env var; `null` when unset. The relay does not proxy or validate it; it is data for the client |
| `kt.mode` | `"none"` (no key-transparency service offered) |
| `min_client_version` | `RELAY_MIN_CLIENT_VERSION` env var; `null` when unset; advisory only |

## Configuration

Three new env vars, matching the `RELAY_TOKEN` env-only precedent. All three
are optional; absent or empty values never fail startup:

- `RELAY_NAME` — operator-set display string for the relay.
- `RELAY_ATTACHMENTS_SERVICE_URL` — attachments service URL to advertise.
- `RELAY_MIN_CLIENT_VERSION` — advisory minimum client version.

## Test Anchors

`tests/na0652_server_info.rs`: probe-vs-full in both auth modes; wrong-token
byte-identity (no oracle); exact-field-set guards on the probe (both nesting
levels) and the full document top level; injected-config value tracking;
end-to-end env plumbing through the real binary.

## Decision

Recorded as qsl-server DECISIONS `D-0012` (wire-contract surface, following
`D-0009`/`D-0010`/`D-0011`). Governance authority: qsl-protocol lane NA-0652,
directive QSL-DIR-2026-07-17-588.
