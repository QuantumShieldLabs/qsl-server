# NEXT_ACTIONS (qsl-server)

### NA-0001 — qsl-server deployment hardening contract (docs-only) + systemd hardening plan

Status: READY
Scope: docs-only (no code/systemd changes yet)

Objective:
- Define the explicit production contract for running qsl-server on AWS safely (TLS termination assumptions, required flags,
  limits, logging/redaction rules, and systemd hardening requirements), so follow-on implementation work is unambiguous.

Invariants:
- No secrets in logs.
- Relay treats payloads as opaque; no payload logging.
- Required resource limits must be explicit (max body bytes, max queue depth, timeouts).
- TLS termination must be explicitly documented (external reverse proxy/ALB) or implemented later under a separate NA.
- Hardening must not change runtime behavior in this NA (docs only).

Deliverables:
- docs/server/DOC-SRV-001_Deployment_Hardening_Contract_v1.0.0_DRAFT.md
- docs/server/DOC-SRV-002_Systemd_Hardening_Plan_v1.0.0_DRAFT.md
- A checklist in NEXT_ACTIONS acceptance criteria for what the follow-on implementation must enforce.

Acceptance criteria:
- Docs exist and are internally consistent with current server flags/options and systemd unit layout.
- TRACEABILITY line added linking NA-0001 to the docs.
- DECISIONS entry added capturing key contract choices (TLS termination model, auth/no-auth stance, required limits).

Evidence:
- N/A (docs-only)
