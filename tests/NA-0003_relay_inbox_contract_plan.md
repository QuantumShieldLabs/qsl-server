# NA-0003 Relay Inbox Contract — Plan

## Scope & assumptions
- Docs-only contract for store-and-forward inbox (PUSH/PULL).
- No server behavior changes in this NA.

## API vectors
- PUSH: channel_id, message_id, ciphertext blob
- PULL: channel_id, max_items

## Limit/overflow vectors
- Oversize message → deterministic reject
- Queue depth exceeded → deterministic reject

## Logging/redaction checks
- No payload logging
- Minimal metadata only (hashed/prefixed identifiers)

## Determinism checks
- Same input → same error codes

## CI commands
- N/A (docs-only)

## Executed evidence (implementation)
- Tests added in src/lib.rs:
  - push_then_pull_roundtrip
  - pull_deletes_on_deliver
  - oversize_returns_413
  - queue_full_returns_429
  - payload_not_logged
- Commands:
  - cargo test
  - cargo clippy --all-targets -- -D warnings

## Rollback
- Revert docs/plan changes only.
