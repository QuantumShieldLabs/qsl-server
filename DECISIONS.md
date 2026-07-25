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

- **ID:** D-0012
  - **Status:** Accepted
  - **Date:** 2026-07-17
  - **Goals:** G4, G5
  - **Decision:** qsl-server gains the ADDITIVE capability document `GET /v1/server-info` (qsl-protocol DOC-PROG-004 step 2, locked decision L4). On a bearer relay an unauthorized request (missing OR wrong token — identical both ways, never a token oracle) receives HTTP 401 with EXACTLY the fixed two-key probe `{"server":"qsl-server","auth":{"mode":"bearer"}}`; the probe registry is FIXED at implementation name + auth mode and may never grow operator config. An authorized request (or any request on an open relay) receives the full document from LIVE configuration: `server`, `version` (CARGO_PKG_VERSION), `name` (`RELAY_NAME`, "" when unset), `api` `["push_v1","pull_v1","pull_ack_lease_v1"]`, `auth.mode` `open|bearer` per `RELAY_TOKEN` presence, `limits.{max_body_bytes,max_queue_depth}`, `retention.ttl_secs`, `directory.mode` `"none"`, `attachments.service_url` (`RELAY_ATTACHMENTS_SERVICE_URL`, null when unset), `kt.mode` `"none"`, `min_client_version` (`RELAY_MIN_CLIENT_VERSION`, null when unset, ADVISORY — not enforced). Contract rules per DOC-SRV-006: additive-only evolution (clients ignore unknown fields; fields never removed/renamed/repurposed); the document gates FEATURES never SECURITY (absence means "not offered", never "not enforced"); served behind the SAME bearer gate as the relay routes; the three existing routes keep plain `ERR_UNAUTHORIZED` byte-identical. `/v1/push`, `/v1/pull`, `/v1/pull/ack` semantics, the storage schema, and the auth mechanism are UNCHANGED; the three new env vars are optional and never fail startup (the `RELAY_TOKEN` env-only precedent).
  - **Rationale:** The GUI onboarding path (DOC-PROG-004 step 5) needs a real "test connection" contract: a client must be able to distinguish "QSL relay, needs auth" from "not a QSL relay" without leaking operator config, and an authorized client must learn the relay's actual limits and features from live values rather than assumptions. Locking additive-only evolution and features-never-security into the spec now prevents the capability document from ever becoming a downgrade vector. Route surface is a recorded-decision area per D-0009/D-0010/D-0011, so the addition is recorded here; governance authority lives in qsl-protocol (NA-0652, D588, D-1275).
  - **References:** qsl-protocol NA-0652 / QSL-DIR-2026-07-17-588 (D588) / D-1275; D-0011; `src/lib.rs`; `docs/server/DOC-SRV-006_Server_Info_Capability_Contract_v1.0.0_DRAFT.md`; `packaging/systemd/relay.env.example`; `packaging/runbook_ubuntu.md`; `tests/na0652_server_info.rs`

- **ID:** D-0013
  - **Status:** Accepted
  - **Date:** 2026-07-18
  - **Goals:** G4
  - **Decision:** qsl-server gains the three community-health files `SECURITY.md`, `CODE_OF_CONDUCT.md`, and `CONTRIBUTING.md`, adapted from the qsl-protocol spine's files as the source of truth (qsl-protocol NA-0655 / QSL-DIR-2026-07-18-591 (D591) / D-1278). `SECURITY.md` carries the spine's reporting section verbatim plus a repo-scope section naming this repository (a research-stage, transport-only relay server) and routing protocol-level reports to qsl-protocol; it describes REPORTING only and adds no assurance claims. `CODE_OF_CONDUCT.md` is byte-identical to the spine's (Contributor Covenant 2.1). `CONTRIBUTING.md` states this repository's real gate (the single required `rust` check on every PR; merge commits only) and the spine-governed lane reality. README, LICENSE, NOTICE, code, tests, dependencies, workflows, and repository settings are untouched by this change.
  - **Rationale:** The org-level `QuantumShieldLabs/.github` defaults already provide generic community-health files and explicitly defer to per-repo files; the per-repo files add the repo-specific scope line and the honest contribution reality the org defaults cannot express. Reviewer outreach (qsl-protocol DOC-PROG-004 parallel track) makes this surface timely. Governance authority lives in qsl-protocol.
  - **References:** qsl-protocol NA-0655 / QSL-DIR-2026-07-18-591 (D591) / D-1278; `SECURITY.md`; `CODE_OF_CONDUCT.md`; `CONTRIBUTING.md`

- **ID:** D-0014
  - **Status:** Accepted
  - **Date:** 2026-07-23
  - **Goals:** G4, G5
  - **Decision:** qsl-server's bearer-token check in `auth_ok` no longer uses `provided == token` (`str::eq`, which short-circuits on the first differing byte). Both sides are now reduced to a fixed 32-byte SHA-256 digest and folded with an XOR-accumulate loop (`diff |= da[i] ^ db[i]` over all 32 bytes, `diff == 0`) in a new private `ct_eq_secret` helper — the same shape as the qsc client handshake's `hs_ct_eq_32` (qsl-protocol ENG-0003) and consistent with this file's existing `route_key_for`, which already hashes the OTHER secret (the route token). Hashing first normalizes both inputs to 32 bytes, so the fold does identical work for every input, closing the length leak as well. The two non-secret `return false` guards (missing / malformed `Authorization` header) are retained — they branch on attacker-known request shape, not on the secret. No new dependency: `sha2` is already a direct dependency of this crate (`subtle` is present in `Cargo.lock` only via rustls under the reqwest DEV-dependency and is deliberately NOT added to the production graph). No wire, API, protocol, schema, or env change; the four gated handlers (`server-info`, `push`, `pull`, `pull/ack`) still reject-before-mutation because `auth_ok` remains their first statement. A same-length wrong-token reject test (`auth_enabled_wrong_token_same_length_401_no_mutation`, `"topsecreX"` vs `"topsecret"`, both 9 bytes) is added because the pre-existing wrong-token test uses different-length tokens, so `==` rejected on length before comparing a byte and passed against the buggy code.
  - **Rationale:** This is the last unfixed HIGH (C-2) from the 2026-07-22 independent audit and the only one outside the governance spine, on the single component deliberately exposed to the network. On a shared bearer token, `str::eq`'s short-circuit is a remote timing oracle; the LAN/tailnet deployment posture is exactly the low-jitter regime in which byte-at-a-time statistical amplification is practical. Fixing it in the file's existing idiom (SHA-256 then fold) makes the relay treat both of its secrets the same way, which is the strongest form of the fix — the property earned is structural (fixed work over the full digest, no data-dependent early return), read-verified, not a measured timing claim. `subtle::ConstantTimeEq` was deliberately not used: it would newly enter the production dependency graph, and — being defined for equal-length slices — would not close the length leak without a length-visible branch, whereas hashing-first closes it for free. HMAC with a random per-process key was not used: its random key defends against precomputation, but an attacker who already holds a candidate token can simply send it, so plain SHA-256 delivers the needed property (comparison time independent of matching-prefix length) with no added machinery. Governance authority lives in qsl-protocol (NA-0670, D606, D-1297).
  - **References:** qsl-protocol NA-0670 / QSL-DIR-2026-07-23-606 (D606) / D-1297; 2026-07-22 independent audit finding C-2; qsl-protocol ENG-0003 (client `hs_ct_eq_32`); `src/lib.rs` (`auth_ok`, `ct_eq_secret`); `TRACEABILITY.md`

- **ID:** D-0015
  - **Status:** Accepted
  - **Date:** 2026-07-25
  - **Goals:** G4
  - **Decision:** Add the operator-infrastructure literal gate and a `cargo audit`
    advisories job to this repository's CI, and extend clippy to `--all-targets`,
    per spine **D613** (NA-0677). **⚠ The intent this came from said "port the
    spine's public-safety job". There was nothing to port:** that job scans for
    private keys and cloud tokens and has never contained an address, path or
    host pattern — which is exactly why it ran green on every pull request that
    published a private LAN address. The failure was the pattern set, not the
    scan's scope. `scripts/ci/infra_literal_scan.py` is that missing pattern set.
  - **The scanner is byte-identical to the copies in `qsl-desktop`,
    `qsl-attachments` and `qsl-protocol`** (`cmp`-proven at landing). The pattern
    set is deliberately **not forked**: one source of truth for one question.
  - **Tiers.** Tier 1 (network-identifying literals and personal identity) over
    the whole tracked tree, failing on any hit. Tier 2b (low-frequency private
    names) over added lines only. Tier 2a (build-root and home paths) not scanned
    at all — the governance convention cites directives by absolute path, so a
    gate on them would be unadoptable. This repository is **Tier-1 clean** at
    landing: 74 files, 13,896 lines examined, zero hits.
  - **The private names are salted SHA-256 digests, not text**, because this
    repository is public and a pattern file naming them would republish what the
    sanitize lane removed — and the Tier-1 scan would then hit its own pattern
    file. The plaintext list is operator-held. Matching is **token-wise**
    (splitting on non-alphanumerics *and* camelCase transitions), so a name
    embedded in an identifier is caught while one merely spanning a camelCase
    seam is not.
  - **Advisories: `cargo audit --deny warnings`, with no waiver file** — this
    repository's dependency graph is clean today (verified at landing). If that
    changes, the fix is a **named-ID** waiver in `.cargo/audit.toml`, never
    dropping `--deny warnings`, which would accept every future unmaintained or
    unsound crate silently.
  - **Clippy `--all-targets`** replaces the lib+bin-only invocation. This
    repository was already clean under it (measured before the change), so this
    is a defensive tightening rather than a fix — it closes the gap by which a
    test-only lint could accumulate unseen.
  - **⚠ Both new jobs are ADVISORY, not blocking.** This repository requires
    exactly one status context, `rust`, which is unchanged. `public-safety` and
    `advisories` run and report but **cannot block a merge** until the operator
    adds them to the required set — a branch-protection change, which is the
    operator's act. Green is not the same as blocking.
  - **A pre-commit call site** (`scripts/hooks/pre-commit`, opt-in via
    `git config core.hooksPath scripts/hooks`) runs the same instrument over the
    staged set. CI is the enforcement; hooks are not cloned.
  - **Proved by positive control in THIS repository**, not inherited from the
    file: a Tier-1 host name embedded as `SOME_<name>_THING` in a tracked file
    makes the scan FAIL, and removing it makes it pass. A gate is a property of
    the repo it runs in, not of the script. Evidence:
    `/srv/qbuild/evidence/NA-0677/gate_positive_control.txt`.
  - **References:** spine D613 (APPROVED 2026-07-25, amended twice; sha256
    `586ae25a…19d57fe0a9b95a51`, 446 lines) and spine NA-0677; qsl-desktop D-0014
    (the first landing, which carries the waiver-file case); spine NA-0676/D-1307
    (the sanitize that made a whole-tree tier adoptable).
