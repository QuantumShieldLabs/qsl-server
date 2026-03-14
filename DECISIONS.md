# DECISIONS (qsl-server)

- **ID:** D-0001
  - **Status:** Accepted
  - **Date:** 2026-01-29
  - **Decision:** Adopt separate governance queue for qsl-server; deployment contract must be documented before behavior changes.
  - **Rationale:** Prevents drift between intended deployment posture and actual operations.
  - **References:** NA-0001
- **ID:** D-0002
  - **Status:** Accepted
  - **Date:** 2026-01-30
  - **Decision:** NA-0002 will enforce explicit limits and logging policy in code; systemd hardening is applied via unit patch per DOC-SRV-002.
  - **Rationale:** Aligns runtime behavior with the deployment contract and reduces operational risk.
  - **References:** NA-0002, DOC-SRV-001, DOC-SRV-002, tests/NA-0002_server_hardening_plan.md
- **ID:** D-0003
  - **Status:** Accepted
  - **Date:** 2026-02-02
  - **Decision:** Define the relay inbox store-and-forward contract before implementing receive behavior.
  - **Rationale:** Ensures ciphertext-only handling, deterministic limits, and explicit retention/auth choices are agreed before code changes.
  - **References:** NA-0003, DOC-SRV-003, tests/NA-0003_relay_inbox_contract_plan.md
- **ID:** D-0004
  - **Status:** Accepted
  - **Date:** 2026-02-08
  - **Decision:** Prioritize optional relay auth gate and panic-removal hardening as release-blocking server hygiene.
  - **Rationale:** Public endpoints need deterministic reject behavior under abuse while preserving current no-auth default when token is unset.
  - **References:** NA-0004, docs/server/DOC-SRV-004_Relay_Auth_And_Hardening_Contract_v1.0.0_DRAFT.md, tests/NA-0004_relay_auth_hardening_plan.md

- **ID:** D-0005
  - **Status:** Accepted
  - **Date:** 2026-02-08
  - **Decision:** Adopt proof-first provenance light-touch controls for qsl-server via `NOTICE`, `PROVENANCE.md`, and a signed-releases runbook.
  - **Rationale:** Operators and users need a clear, repeatable path to verify source authenticity and release provenance without changing server behavior.
  - **References:** NA-0005, tests/NA-0005_provenance_lighttouch_plan.md

- **ID:** D-0006
  - **Status:** Accepted
  - **Date:** 2026-03-14
  - **Decision:** Canonical qsl-server deployment/install/update path is packaging-based: `packaging/systemd/qsl-server.service`, `packaging/systemd/relay.env.example`, `/etc/qsl-server/relay.env`, `scripts/install_ubuntu.sh`, `scripts/update_ubuntu.sh`, `scripts/update_from_release.sh`, `scripts/aws_update_and_verify.sh`, and `scripts/verify_remote.sh` using deploy metadata instead of an on-host git checkout.
  - **Rationale:** The public README and Ubuntu runbook already point at the packaging unit/env path; keeping the legacy root unit and build-on-host installer as active alternatives creates deployment drift without adding runtime value.
  - **References:** NA-0006, README.md, packaging/runbook_ubuntu.md, packaging/systemd/qsl-server.service, scripts/install_ubuntu.sh, scripts/update_ubuntu.sh, scripts/update_from_release.sh, scripts/aws_update_and_verify.sh, scripts/verify_remote.sh, scripts/ci/test_canonical_packaging_alignment.sh

- **ID:** D-0007
  - **Status:** Accepted
  - **Date:** 2026-03-13
  - **Goals:** G4, G5
  - **Decision:** qsl-server workflow maintenance will upgrade maintained GitHub-managed actions to safe maintained majors and establish a minimal `main` branch-protection baseline that requires only the ordinary PR safety context `rust`. Tag/release-only workflows are not required on ordinary pull requests.
  - **Rationale:** qsl-server has only one meaningful PR-safety lane today (`ci.yml` / `rust`), while `release-linux.yml` is tag-only and should not be part of ordinary PR gating. A minimal truthful baseline reduces branch-protection drift without widening into runtime or release-process redesign.
  - **References:** NA-0007; `.github/workflows/ci.yml`; `.github/workflows/release-linux.yml`; `TRACEABILITY.md`

- **ID:** D-0008
  - **Status:** Accepted
  - **Date:** 2026-03-14
  - **Goals:** G4, G5
  - **Decision:** The current qsl-server route-token-in-URL API shape is retained only as the current compatibility surface and should migrate away from URL-embedded route tokens. This directive records the threat model and migration requirements but does not change runtime behavior.
  - **Rationale:** Route tokens are capability-like identifiers that currently propagate in request URIs across operator-visible surfaces such as reverse-proxy logs, shell command lines, screenshots, and observability traces. The existing deployment guidance already needs `/v1/*` log suppression, which is evidence that KEEP would leave too much safety burden on compensating controls.
  - **References:** NA-0008; `README.md`; `docs/server/DOC-SRV-003_Relay_Inbox_Contract_v1.0.0_DRAFT.md`; `docs/server/DOC-SRV-005_Route_Token_API_Shape_Review_v1.0.0_DRAFT.md`; `TRACEABILITY.md`
