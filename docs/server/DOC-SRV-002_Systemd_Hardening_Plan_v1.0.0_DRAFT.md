# DOC-SRV-002 — Systemd Hardening Plan (DRAFT)

## Purpose
Define systemd hardening requirements for qsl-server deployment. This is a plan only; actual unit changes must be implemented under a
follow-on NA after review.

## Run-as user guidance
- Create a dedicated user/group (e.g., `qsl-server:qsl-server`).
- Working directory should be owned by this user and not writable by others.
- Runtime state directory should be owned by this user with 0700 permissions.

## Recommended hardening stanza (plan)
Add these settings to the systemd unit where compatible:

```
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/qsl-server
CapabilityBoundingSet=
AmbientCapabilities=
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=true
LockPersonality=true
MemoryDenyWriteExecute=true
SystemCallFilter=@system-service
```

Notes:
- `ReadWritePaths` should match the actual runtime state path used by the service.
- `SystemCallFilter` may need adjustments if the server uses additional syscalls; validate in staging first.

## Logging hardening
- Configure journald rate limits to avoid log flooding.
- Ensure logs do not include payload bytes or secrets (see DOC-SRV-001).

## Environment and limits
- `Environment=PORT=...`
- `Environment=MAX_BODY_BYTES=...`
- `Environment=MAX_QUEUE_DEPTH=...`
- `LimitNOFILE=...` (TBD: set based on expected concurrency)

## Rollout plan
1) Apply hardening in staging first.
2) Verify health checks, request throughput, and log behavior.
3) If failures occur, rollback to the previous unit configuration and document the incompatibility.

## Rollback steps
- Revert to the previous known-good unit file.
- `systemctl daemon-reload`
- `systemctl restart qsl-server`
- Re-run verify script to confirm service health.
