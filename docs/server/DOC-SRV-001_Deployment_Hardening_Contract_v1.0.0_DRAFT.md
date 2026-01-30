# DOC-SRV-001 — Deployment Hardening Contract (DRAFT)

## Purpose
Define the production deployment contract for qsl-server before any behavior changes. This contract is the required baseline for any
future hardening or feature work.

## Service model
- **Service role:** transport-only relay that stores/forwards opaque payloads.
- **Protocol boundary:** no protocol parsing, no cryptography, no payload inspection.
- **Interface:** HTTP API (see README.md for endpoints).

## TLS termination model (explicit)
- **Current model:** qsl-server is HTTP-only; TLS termination MUST occur upstream (ALB, Nginx, or equivalent).
- **Requirement:** qsl-server MUST NOT be directly internet-facing without TLS termination and strict network ACLs.
- **Follow-on:** If end-to-end TLS inside qsl-server is required, it MUST be implemented under a separate NA.

## Network exposure
- **Bind default:** 0.0.0.0 is permitted only behind a restricted Security Group / firewall.
- **Recommended bind:** 127.0.0.1 when placed behind a local reverse proxy on the same host.
- **Inbound port:** default 8080 (PORT env/--port). Public exposure MUST be explicitly approved.

## Required runtime flags + limits
These are required for production deployments. Values below are recommended ceilings unless overridden by a documented exception.

- `MAX_BODY_BYTES`
  - **Default:** 1,048,576 (1 MiB)
  - **Recommended ceiling:** 1 MiB (do not exceed without review)
  - **Rationale:** reduces memory pressure and mitigates abuse
- `MAX_QUEUE_DEPTH`
  - **Default:** 256
  - **Recommended ceiling:** 256 (do not exceed without review)
  - **Rationale:** prevents unbounded queue growth
- `PORT`
  - **Default:** 8080
  - **Allowed range:** 1024–65535 (avoid 80/443 at the app layer if TLS is terminated upstream)

### Timeouts
- **Current code:** no explicit request/idle timeouts configured.
- **Contract requirement:** upstream proxy or load balancer MUST enforce:
  - request header timeout
  - request body timeout
  - idle connection timeout
- **TBD:** if timeouts are needed in-app, define and implement under a separate NA.

## Logging policy (strict)
- **No payload logging:** payload bytes MUST never be logged.
- **No secret logging:** do not log keys, tokens, or credentials.
- **Markers only:** operational logs should be limited to addresses, status, and bounded metadata.
- **IP logging:** avoid full client IP logging unless required for abuse mitigation; prefer sampled logs.

## Authentication policy
- **Current stance:** no authentication at relay layer.
- **Compensating controls required:**
  - strict network ACLs (Security Group rules)
  - enforced size/queue limits
  - optional upstream rate limiting
- **Future auth:** if authentication is required, it must be introduced in a separate NA with explicit threat model.

## Operational checklist (deployment)
- [ ] TLS termination configured upstream (ALB/Nginx) with HTTPS-only ingress
- [ ] Security group/firewall restricts inbound traffic to approved sources
- [ ] `MAX_BODY_BYTES` set explicitly (≤ 1 MiB unless exception approved)
- [ ] `MAX_QUEUE_DEPTH` set explicitly (≤ 256 unless exception approved)
- [ ] Health checks configured (HTTP 200/204 behavior documented)
- [ ] Logs reviewed to ensure no payload/secret leakage
- [ ] Systemd hardening plan applied per DOC-SRV-002 (implementation NA)

## Observability
- **Safe metrics:** request counts, response codes, queue depth, size limit hits
- **Unsafe metrics:** any payload content or identifiers derived from payload

## Change control
Any deviation from this contract requires:
- a documented exception with rationale
- a follow-on NA for implementation changes

## Implementation Notes / Deploy Checklist (NA-0002)
Use this checklist when applying NA-0002 changes:
- [ ] Update systemd unit per DOC-SRV-002 (hardening stanza) and reload systemd.
- [ ] Set explicit environment values in the unit:
  - MAX_BODY_BYTES (<= 1 MiB recommended ceiling)
  - MAX_QUEUE_DEPTH (<= 256 recommended ceiling)
  - PORT (default 8080; avoid 80/443 at app layer)
- [ ] Verify service health with the repo verify script (if available).
- [ ] Confirm logs contain no payload bytes or secrets (grep guard).
- [ ] Rollback plan documented (restore prior unit file, restart service).
