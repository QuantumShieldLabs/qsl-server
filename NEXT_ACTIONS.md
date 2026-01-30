# NEXT_ACTIONS (qsl-server)

### NA-0001 — qsl-server deployment hardening contract (docs-only) + systemd hardening plan

Status: DONE
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
- PR #4 (https://github.com/QuantumShieldLabs/qsl-server/pull/4) merged (merge SHA 5f1ebe9c156f2faa3acc1bce7d66b5b0679bbe01).
- N/A (docs-only)

### NA-0002 — Enforce deployment contract in server + systemd hardening patch (implementation)

Status: READY
Scope: server code + systemd unit patch (implementation PR), plus tests where feasible.
Wire/behavior change allowed? YES (limits enforcement + safer defaults)
Objective:
- Implement DOC-SRV-001 contract requirements in code/config:
  * enforce max-body-bytes / queue-depth ceilings
  * ensure payloads are opaque and never logged
  * ensure safe bind/port defaults are explicit
- Provide a hardened systemd unit patch per DOC-SRV-002 (NoNewPrivileges/ProtectSystem/etc.)
- Produce a deploy checklist for AWS instance updates (manual steps; Codex does not deploy)

Invariants:
- No secrets or payload contents in logs.
- Server rejects oversized payloads deterministically (413 or equivalent) without crashing.
- Queue depth is bounded; overflow behavior is deterministic.
- Defaults are safe and documented; production must not rely on implicit unlimited settings.
- Systemd hardening does not break service startup (verified in staging steps; at least lint/unit-file validation in repo).

Deliverables:
- Code changes implementing limits and logging policy.
- Update systemd/qsl-server.service (or provide a .patch file under systemd/) with hardening stanza.
- Tests:
  * oversize payload rejected (integration/unit)
  * queue depth bound enforced
- Docs update:
  * add “Implementation Notes / Deploy Checklist” section to DOC-SRV-001 or new DOC-SRV-003 checklist.
- CI evidence: cargo test + clippy clean for qsl-server (or whatever lint gates exist).

Acceptance:
- Tests prove oversize reject + bounded queue behavior.
- No payload logging confirmed via tests or grep guard.
- systemd unit hardening patch included and documented.
- TRACEABILITY updated with PR links and artifacts.
