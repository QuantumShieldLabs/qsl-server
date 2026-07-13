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

- **ID:** D-0009
  - **Status:** Accepted
  - **Date:** 2026-03-14
  - **Goals:** G4, G5
  - **Decision:** qsl-server now makes `X-QSL-Route-Token` on token-free `POST /v1/push` and `GET /v1/pull?max=N` the canonical route-token carriage mechanism while preserving legacy `/v1/push/:channel` and `/v1/pull/:channel?max=N` during a compatibility window. Legacy path/header requests are accepted only when the values match; mismatches reject deterministically with no queue mutation.
  - **Rationale:** Header carriage removes route tokens from the most widely propagated request surface without overloading `Authorization` or redesigning `GET` request bodies. Keeping the legacy path during the compatibility window avoids a silent break for existing clients/operators while allowing supported flows to move immediately to a safer canonical shape.
  - **References:** NA-0009; `src/lib.rs`; `README.md`; `docs/server/DOC-SRV-003_Relay_Inbox_Contract_v1.0.0_DRAFT.md`; `docs/server/DOC-SRV-005_Route_Token_API_Shape_Review_v1.0.0_DRAFT.md`; `packaging/runbook_ubuntu.md`; `scripts/verify_remote.sh`; `scripts/aws_update_and_verify.sh`; `tests/relay_smoke.rs`; `scripts/ci/test_route_token_migration.sh`; `TRACEABILITY.md`

- **ID:** D-0010
  - **Status:** Accepted
  - **Date:** 2026-03-30
  - **Goals:** G4, G5
  - **Decision:** qsl-server now retires legacy `/v1/push/:channel` and `/v1/pull/:channel?max=N` outright. Canonical header-carried routing on token-free `/v1/push` and `/v1/pull?max=N` is the only supported route-token ingress shape.
  - **Rationale:** The compatibility window from D-0009 existed only to get supported clients and operator guidance onto the safer header-based posture. That migration is now complete enough that continuing to accept URI-carried route tokens leaves a known passive-leak surface live without adding truthful transport value.
  - **References:** NA-0012; `src/lib.rs`; `tests/relay_smoke.rs`; `README.md`; `docs/server/DOC-SRV-003_Relay_Inbox_Contract_v1.0.0_DRAFT.md`; `packaging/runbook_ubuntu.md`; `scripts/check_relay_compatibility.sh`; `scripts/verify_remote.sh`; `scripts/aws_update_and_verify.sh`; `scripts/ci/test_relay_deploy_compatibility_guard.sh`; `TRACEABILITY.md`

- **ID:** D-0011
  - **Status:** Accepted
  - **Date:** 2026-07-13
  - **Goals:** G4, G5
  - **Decision:** qsl-server's store-and-forward queue becomes DURABLE (embedded SQLite via rusqlite/bundled, WAL + synchronous=FULL, single-file store at the required `STORE_PATH`; route tokens persisted only as SHA-256 digests; payloads stored verbatim as opaque blobs), and the delivery contract adds an ACKNOWLEDGED-PULL mode per the qsl-protocol D578 design-lock (option B, operator-chosen): `GET /v1/pull?ack=lease` returns messages WITHOUT deleting and leases them for `PULL_LEASE_SECS` (default 60 s); `POST /v1/pull/ack {"ids":[...]}` deletes ONLY leased copies (idempotent; scoped to the route; unleased duplicate copies per the NA-0275 contract survive); un-acked leases expire and the messages reappear. The LEGACY pull (`GET /v1/pull?max=N`, no ack parameter) keeps its exact delete-on-deliver contract and response shape — the current non-acking qsc client is not stranded. The 5-minute idle-route discard (`ROUTE_IDLE_TTL_MS`) is RETIRED (warn-and-ignore) and replaced by an operator-tunable retention TTL for undelivered messages (`RETENTION_TTL_SECS`, default 7 days, ceiling 30 days); delivered+acked messages are still forgotten immediately — the relay is reliable, not an archive. Startup is fail-closed: `STORE_PATH` has no default.
  - **Rationale:** The in-memory queue was demo-class: a restart dropped every queued message and idle routes discarded after 5 minutes, unacceptable for the DOC-PROG-003 self-host operator-path (Tier-1, step 1). Delete-on-pull loses a message if the puller crashes between pull and local persistence; the lease model closes that window without wire-semantic or E2EE change (payloads stay opaque, the relay stays blind, nothing precludes future E2EE read receipts as ordinary payloads). Route/pull semantics are a recorded-decision surface per D-0009/D-0010, so the contract change is recorded here; governance authority for the lane lives in qsl-protocol (NA-0642, D578, D-1265).
  - **References:** qsl-protocol NA-0642 / QSL-DIR-2026-07-13-578 (D578) / D-1265; D-0009; D-0010; `src/store.rs`; `src/lib.rs`; `src/main.rs`; `packaging/systemd/relay.env.example`; `packaging/systemd/qsl-server.service` (StateDirectory); `packaging/runbook_ubuntu.md`; `tests/na0642_durability_restart.rs`; `tests/na0642_ack_contract.rs`; `tests/na0642_retention_lifecycle.rs`; `tests/na0642_retention_logging.rs`; `tests/na0642_backward_compat.rs`; `tests/na0642_concurrency.rs`; `tests/na0642_store_privacy.rs`
