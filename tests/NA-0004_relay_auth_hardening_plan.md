# NA-0004 Relay Auth Hardening Plan

## Scope

- Add optional bearer-token auth gate to relay push/pull.
- Remove panic/unwrap runtime paths from startup and handlers.
- Preserve deterministic error behavior and no-mutation reject semantics.

## API vectors

- Auth disabled (`RELAY_TOKEN` unset): push/pull continue to work.
- Auth enabled (`RELAY_TOKEN` set):
  - missing `Authorization` => `401 ERR_UNAUTHORIZED`
  - wrong bearer token => `401 ERR_UNAUTHORIZED`
  - correct bearer token => push/pull succeed

## Limit/overflow vectors

- Oversize body => `413 ERR_TOO_LARGE`
- Queue depth exceeded => `429 ERR_QUEUE_FULL`

## Logging-redaction checks

- No payload bytes in logs under success/failure paths.
- No secrets/tokens echoed in error bodies or logs.

## Determinism checks

- Unauthorized, oversize, and queue-full responses are stable (status + error code).
- Reject paths do not mutate queue state.

## CI commands

- `cargo fmt --check`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`

## Rollback

- Revert NA-0004 implementation PR if auth gate or panic-removal breaks compatibility.
- Keep docs and traceability references for audit trail.
