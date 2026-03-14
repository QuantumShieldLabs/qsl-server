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

Status: DONE
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

Evidence:
- Impl PR #7 (https://github.com/QuantumShieldLabs/qsl-server/pull/7) merged (merge SHA 13c7266817f158ce9ca5a786eb540e7d4453083c).

### NA-0003 — Relay inbox store-and-forward contract (PUSH/PULL) + test plan (docs-only)

Status: DONE
Scope: docs-only (no code changes)

Invariants:
- Ciphertext-only; no payload logging.
- Hard limits enforced: MAX_BODY_BYTES, MAX_QUEUE_DEPTH; deterministic rejects.
- Bounded retention (TTL) documented.
- Minimal metadata: opaque channel IDs only; no usernames.

Deliverables:
- docs/server/DOC-SRV-003_Relay_Inbox_Contract_v1.0.0_DRAFT.md
- tests/NA-0003_relay_inbox_contract_plan.md

Acceptance:
- Doc exists and is internally consistent.
- Plan stub exists.
- TRACEABILITY updated with PR link.
- READY count exactly 1.

Evidence:
- Impl PR #10 (https://github.com/QuantumShieldLabs/qsl-server/pull/10) merged (merge SHA 0649094990bcd81db61d37c2f91d73653a12d907).

### NA-0004 — Server hardening: optional auth gate + remove unwrap/panic paths + deterministic rejects + test-backed

Status: DONE
Scope: server code + tests (implementation PR later). No protocol/client changes.

What is protected:
- Public relay reliability and abuse resistance.
- Deterministic behavior under malformed/hostile input.

Invariants:
1) Optional auth gate:
   - If `RELAY_TOKEN` is configured, push/pull MUST require `Authorization: Bearer <token>`.
   - Missing/invalid token => deterministic `401 ERR_UNAUTHORIZED` (no mutation).
   - If `RELAY_TOKEN` is NOT configured, auth remains disabled (current behavior).
2) No panics/unwraps in runtime paths (startup and handlers); deterministic errors instead.
3) No payload logging (maintained invariant).
4) Bounded limits remain enforced (`413` too large, `429` queue full).

Acceptance:
- Tests cover:
  - auth disabled => push/pull ok
  - auth enabled => missing token `401`, wrong token `401`, correct token ok
  - no unwrap/panic paths remain (clippy/grep + tests)
- `cargo fmt`, `cargo test`, `cargo clippy` pass; CI (if present) green.

Evidence:
- Impl PR #13 (https://github.com/QuantumShieldLabs/qsl-server/pull/13) merged (merge SHA 1fa1978f2f30057f34a139d2f01e1e1c61746d96).

### NA-0005 — Provenance light touch: NOTICE + PROVENANCE + signed-release runbook

Status: DONE
Scope: governance files + root docs only (no src/systemd/workflow changes)

Objective:
- Add a lightweight provenance baseline for qsl-server users:
  - repository notice + licensing pointer
  - official-source and verification guidance
  - signed-tag/checksum runbook instructions

Invariants:
1) No server runtime behavior changes.
2) No `src/**`, `systemd/**`, `scripts/**`, `.github/**`, `Cargo.toml`, or `Cargo.lock` edits.
3) Guidance must be proof-first and fail-closed: do not trust unaudited binaries.
4) Auth posture statement remains aligned with implemented server behavior (`Authorization: Bearer` required only when `RELAY_TOKEN` is configured).

Deliverables:
- Add `NOTICE` at repo root.
- Add `PROVENANCE.md` at repo root.
- Add `SIGNED_RELEASES_RUNBOOK.md` at repo root.
- Update `tests/NA-0005_provenance_lighttouch_plan.md` with executed evidence.
- Implementation complete: PR #16 (https://github.com/QuantumShieldLabs/qsl-server/pull/16), merge SHA `cfc8e600988dbba40f30a39eda7021efaf8c83d5`.

Acceptance:
- Governance, implementation, and close-out PRs are scope-limited and green.
- TRACEABILITY contains READY/implementation/DONE links and merge SHAs.
- Queue returns to READY=0 after close-out.

### NA-0006 — Deployment/Layout Cleanup + Canonical Packaging Alignment

Status: DONE
Scope: packaging/systemd/docs/scripts only (no `src/**`, no relay semantics, no API/auth changes)

Problem:
- The repo already documents packaging-based install/update flow, but conflicting legacy service/layout artifacts still exist and make the canonical deploy path ambiguous.

Invariants:
1) No `src/**`, `Cargo.toml`, `Cargo.lock`, or `.github/workflows/**` changes.
2) No relay runtime semantic, API shape, auth behavior, or protocol parsing changes.
3) Canonical service/env/install/update path must be packaging-based:
   - `packaging/systemd/qsl-server.service`
   - `packaging/systemd/relay.env.example`
   - deployed env file `/etc/qsl-server/relay.env`
   - `scripts/install_ubuntu.sh`
   - `scripts/update_ubuntu.sh`
   - `scripts/update_from_release.sh`
   - `scripts/aws_update_and_verify.sh`
4) Duplicate or conflicting legacy artifacts must be removed or clearly marked deprecated and kept out of the canonical path.
5) Verification must not assume `/opt/qsl-server/repo` exists on the deployed host.

Deliverables:
- Align README/runbook/install/update/verify scripts to one canonical packaging path.
- Remove or deprecate conflicting legacy service/install artifacts.
- Add deterministic alignment checks proving the canonical path and legacy-artifact handling.
- Update TRACEABILITY/DECISIONS with the implementation evidence.

Acceptance:
- One canonical in-tree deployment/layout path is represented consistently in docs/scripts.
- `verify_remote.sh` verifies a canonical deployed host without assuming `/opt/qsl-server/repo`.
- Any retained legacy artifact is clearly marked deprecated and not referenced by canonical docs/scripts/tests.
- Queue returns to READY=0 after close-out.

Evidence:
- PR: #27 https://github.com/QuantumShieldLabs/qsl-server/pull/27
- Merge SHA: `94da6e22eac7`
- mergedAt: `2026-03-14T02:16:44Z`
- Outcomes:
  - Canonical install/update path is packaging-based: `packaging/systemd/qsl-server.service`, `packaging/systemd/relay.env.example`, `/etc/qsl-server/relay.env`, `scripts/install_ubuntu.sh`, `scripts/update_ubuntu.sh`, `scripts/update_from_release.sh`, `scripts/aws_update_and_verify.sh`, and `scripts/verify_remote.sh`.
  - Conflicting root `systemd/qsl-server.service` was removed, and `scripts/install_ubuntu_24_04_systemd.sh` now remains only as a deprecated wrapper to the canonical installer.
  - `verify_remote.sh` now reads deploy metadata from `/opt/qsl-server/DEPLOYMENT_INFO` instead of assuming `/opt/qsl-server/repo`, and deterministic script checks prove the canonical alignment.
- Evidence hygiene:
  - No relay/API/auth/runtime semantics changed; no secrets, bearer values, or token-bearing env contents were committed.

### NA-0007 — Actions Runtime Maintenance + Protection Baseline

Status: DONE
Scope: workflow/policy/settings only (`.github/workflows/**`, minimal CI-only support, governance linkage)

Problem:
- qsl-server's maintained public workflows still depend on deprecation-exposed GitHub Action majors, and `main` still lacks a truthful branch-protection baseline for the repo's actual PR safety surface.

Invariants:
1) No `src/**`, `Cargo.toml`, `Cargo.lock`, runtime semantics, auth posture, or API-shape changes.
2) Maintain only the meaningful PR safety context(s) on `main`; do not require tag/release-only workflows on ordinary PRs.
3) Required checks must always resolve without deadlocking on skipped workflows.

Deliverables:
- Update maintained qsl-server workflows to safe maintained action majors where available.
- Establish a minimal truthful branch-protection baseline on `main`.
- Record the protection/runtime-maintenance decision in `DECISIONS.md` / `TRACEABILITY.md`.

Acceptance:
- Maintained workflows no longer emit the current JS-action runtime deprecation warning.
- `main` branch protection exists and requires only the meaningful ordinary-PR safety context(s).
- Queue returns to READY=0 after close-out.

Evidence:
- PR: #30 https://github.com/QuantumShieldLabs/qsl-server/pull/30
- Merge SHA: `e61239ff84b2`
- mergedAt: `2026-03-14T03:42:29Z`
- Outcomes:
  - Maintained qsl-server workflows now use `actions/checkout@v5` and `actions/upload-artifact@v6` where applicable; `dtolnay/rust-toolchain@stable` remains unchanged because it is a composite action, not a deprecation-exposed JS action.
  - `main` branch protection now exists with admin enforcement enabled and requires only the ordinary PR safety context `rust`; the tag-only `release-linux` workflow is not required on pull requests.
  - No `src/**`, `Cargo.toml`, `Cargo.lock`, auth, API, or relay semantic changes occurred in this item.
- Evidence hygiene:
  - Workflow/policy/settings scope only; no secrets, tokens, bearer values, or capability-bearing URLs were committed.

### NA-0008 — Route-Token API Shape Review + Migration Decision

Status: DONE
Scope: docs/design only (`README.md`, `docs/**`, `DECISIONS.md`, `TRACEABILITY.md`; no `src/**`, no `Cargo.*`, no workflows, no runtime/API/auth changes)

Problem:
- qsl-server still documents relay route tokens in `/v1/push/{channel}` and `/v1/pull/{channel}` URL paths, making capability-like identifiers operator-visible in request URIs and pushing log-safety onto deployment/operator compensating controls.

Invariants:
1) Docs/design only; no `src/**`, `Cargo.toml`, `Cargo.lock`, or `.github/workflows/**` changes.
2) No runtime, API-shape, auth, or relay-semantic changes in this item.
3) No token disclosure in docs, examples, evidence, or operator guidance.
4) No silent compatibility break; if migration is chosen, follow-on requirements must define compatibility and rollout.

Deliverables:
- Threat-model the current route-token-in-URL shape with grounded leakage surfaces.
- Decide KEEP vs MIGRATE and record the rationale in qsl-server docs/design artifacts.
- If MIGRATE is chosen, define the direct implementation follow-on requirements without implementing them here.

Acceptance:
- Decision is recorded with rationale and grounded leakage surfaces.
- Operator/docs impacts are explicit and secret-safe.
- Queue returns to READY=0 after close-out.

Evidence:
- PR: #33 https://github.com/QuantumShieldLabs/qsl-server/pull/33
- Merge SHA: `893144a5a5e9`
- mergedAt: `2026-03-14T12:16:35Z`
- Decision: MIGRATE
- Outcomes:
  - qsl-server now records the route-token threat model in `docs/server/DOC-SRV-005_Route_Token_API_Shape_Review_v1.0.0_DRAFT.md` and in Decision D-0008.
  - README and `DOC-SRV-003` now explicitly treat `/v1/push/:channel` and `/v1/pull/:channel` as the current compatibility shape only, not the desired end state.
  - The direct follow-on requirements are defined docs-first: compatibility window or equivalent safe transition, log-safety requirements, operator/runbook changes, and deterministic validation expectations.
- Evidence hygiene:
  - Docs/design scope only; no `src/**`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/**`, runtime/API/auth behavior, or relay semantics changed, and no raw route tokens were committed.
