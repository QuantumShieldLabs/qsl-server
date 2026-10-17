Read PROJECT_WORKFLOW.md. These commands are an operator reference for an explicitly authorized release after all applicable hardening triggers and release checks are closed. A verified tag/checksum proves identity and integrity, not security or release qualification. No tagging, publishing or deployment follows from reading this runbook.

# Signed Releases Runbook (qsl-server)

This runbook describes user/operator steps for signed tags and release checksum verification.

No keys are generated or stored by this repository.

## Prerequisites

- Git with signing configured (GPG or SSH signing).
- Maintainer permissions to push tags.
- Clean working tree before tagging.

## 1) Create and verify a signed tag

```bash
git checkout main
git pull --ff-only
git status --porcelain
git tag -s vX.Y.Z -m "qsl-server vX.Y.Z"
git tag -v vX.Y.Z
git push origin vX.Y.Z
```

## 2) Generate and verify release checksums

```bash
sha256sum qsl-server-linux-amd64 qsl-server-linux-arm64 > SHA256SUMS
sha256sum -c SHA256SUMS
```

Optional detached signature for checksum manifest:

```bash
gpg --armor --detach-sign SHA256SUMS
gpg --verify SHA256SUMS.asc SHA256SUMS
```

## 3) Consumer verification

```bash
git fetch --tags origin
git tag -v vX.Y.Z
sha256sum -c SHA256SUMS
```

Accept artifacts only when:
- signed tag verification succeeds,
- checksum verification succeeds,
- commit/run evidence is traceable in `TRACEABILITY.md`.
