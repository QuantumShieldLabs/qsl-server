# NA-0002 — qsl-server contract enforcement + systemd hardening plan

## Scope & assumptions
- Enforce DOC-SRV-001 contract in code and config.
- Apply DOC-SRV-002 hardening via systemd unit patch (implementation PR).

## Contract requirements mapping
- Limits: MAX_BODY_BYTES, MAX_QUEUE_DEPTH
- Logging: no payload/secret logging
- Deterministic reject behavior

## Test vectors
- Oversize payload rejection (413)
- Queue depth overflow (429)

## Logging redaction checks
- Grep logs to ensure payload bytes never appear.

## Systemd hardening verification
- Unit file validation/lint
- Manual staging checklist

## CI commands
- cargo test
- cargo clippy

## Rollback
- Revert unit patch, restart service, re-run smoke test
