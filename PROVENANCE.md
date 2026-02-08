# qsl-server Provenance

This document defines how to verify official qsl-server source and deployment artifacts.

## What "official" means

- Source of truth is `main` in:
  - https://github.com/QuantumShieldLabs/qsl-server
- Governance and evidence are maintained in:
  - `NEXT_ACTIONS.md`
  - `TRACEABILITY.md`
  - `DECISIONS.md`
- Changes are expected to pass repository CI before merge.

## Verify source identity for a checkout

From repository root:

```bash
git remote -v
git rev-parse HEAD
git log -1 --oneline
```

Confirm the commit SHA is linked in merged PR evidence in `TRACEABILITY.md`.

## Verify deployment identity (systemd + binary checksum)

On the deployment host:

```bash
systemctl cat qsl-server.service
systemctl status qsl-server --no-pager
sha256sum /path/to/qsl-server-binary
```

Compare the service unit content and binary checksum to your recorded release notes and `SHA256SUMS`.

## Auth and logging posture

- Auth gate is optional and configuration-driven:
  - if `RELAY_TOKEN` is configured, requests require `Authorization: Bearer <token>`.
  - if `RELAY_TOKEN` is not configured, auth remains disabled.
- Relay payloads are treated as opaque; payload bodies should not be logged.
- Ciphertext-only expectation: the relay stores/transports ciphertext envelopes and should not inspect plaintext content.

## Artifact trust model

- Trust source commit + release checksum + CI evidence together.
- Treat detached binaries without source/commit/run linkage as untrusted.
