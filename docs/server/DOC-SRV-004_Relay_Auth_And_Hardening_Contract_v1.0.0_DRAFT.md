# DOC-SRV-004 Relay Auth And Hardening Contract v1.0.0 (DRAFT)

## Purpose

Define server-side hardening requirements for relay push/pull endpoints:
- optional bearer-token auth gate
- deterministic error contract
- panic/unwrap removal in runtime paths
- no payload logging

## Auth Contract

- Auth is controlled by `RELAY_TOKEN`.
- If `RELAY_TOKEN` is unset/empty:
  - auth is disabled
  - behavior remains compatible with current open relay mode
- If `RELAY_TOKEN` is set:
  - `Authorization` header is required
  - accepted format: `Authorization: Bearer <token>`
  - missing/invalid token => `401` with deterministic code `ERR_UNAUTHORIZED`
  - reject path must not mutate relay queue state

## Deterministic Error Codes

- Unauthorized: `401 ERR_UNAUTHORIZED`
- Request too large: `413 ERR_TOO_LARGE`
- Queue full: `429 ERR_QUEUE_FULL`
- Handler/startup failures from parse/IO/config must map to deterministic non-panic errors.

## Runtime Hardening

- No `unwrap`/`expect` in runtime execution paths (startup + handlers).
- Use explicit error propagation and deterministic responses.
- Keep payloads opaque; do not parse or log ciphertext payload bytes.

## Deployment Notes

- Compatible with Caddy/reverse-proxy TLS termination in front of relay.
- Relay remains ciphertext-only and metadata-minimal.
- Auth token should be injected via environment management (systemd env file / secret manager), not hard-coded.
