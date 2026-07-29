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
    - **⚠ SUPERSEDED 2026-07-26 (NA-0678, D614 §4b / OBS-BM). The paragraph above
      was true at landing and is false now.** The operator promoted all three
      contexts at the NA-0677 closeout: this repository's required set is
      `["rust", "public-safety", "advisories"]`, `strict: true`,
      `enforce_admins: true` (read back from the API at D614 drafting). The
      original text is kept legible rather than rewritten, per the house rule
      that a superseded passage stays readable — but it must not be relied on:
      **both jobs now block.**
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

- **ID:** D-0016
  - **Status:** Accepted
  - **Date:** 2026-07-26
  - **Goals:** G1, G4
  - **Decision:** qsl-server gains the ADDITIVE invite-slot subsystem (messaging epic Slice 1; qsl-protocol NA-0678 / QSL-DIR-2026-07-26-614 (D614)): three routes `POST /v1/invite/create`, `POST /v1/invite/redeem`, `POST /v1/invite/revoke`, a slot-scoped admission check on the existing `POST /v1/push`, an `invites` table at `SCHEMA_VERSION` 2, and five new resource controls. **The CLIENT mints the redemption capability and uploads only `SHA-256(cap)`; there is deliberately NO mint endpoint**, so no relay-side path holds a capability in plaintext before a redeemer presents one and a relay operator cannot silently burn an invite it hosts. All three routes are POSTs carrying `invite_id` and every secret in the JSON **body**, never a path or query parameter — `invite_id` IS the mailbox route key, and D-0008/D-0009/D-0010 already retired URI-carried route tokens for exactly this reason. `invite_id`, `cap`, `revoke_token` and the handshake `ticket` are persisted only as SHA-256 digests and compared with the existing `ct_eq_secret` (D-0014) — **no new primitive**. Redemption is an atomic compare-and-set (exactly one winner; every loser gets `ERR_INVITE_ALREADY_USED`) and issues a **one-shot handshake ticket**, without which a push to an invite slot is refused — so the slot accepts exactly one handshake, from the party that actually redeemed rather than from anyone who saw the code and lost the race. A 128-bit `revoke_token` returned once at create authorizes revoke, which is idempotent; without it an open relay would let any code-holder destroy any invite. Consumed and revoked slots are **TOMBSTONED until expiry** with their blobs cleared, so `invite-already-used` stays distinguishable from `invite-not-found` — a deleted slot would report "never existed" when the truth is "someone got here first". The relay stores `bundle` and `invite_sig` as **opaque bytes** and never parses them. `MAX_INVITE_SLOTS` (256/4096) bounds storage and **never evicts** when full; a **GLOBAL** `INVITE_CREATE_BURST`/`INVITE_CREATE_REFILL_PER_SEC` bucket bounds denial — global because an invite has no route token until it exists, so the per-route push bucket structurally cannot cover it. The two are **not substitutes**: operator ruling, *the availability of invite-create is a security property; slot-cap-only is a DoS*. `GET /v1/server-info` gains `invite_v1`, `limits.max_invite_bundle_bytes` and an `invite` object, additively per DOC-SRV-006. **`/v1/push`, `/v1/pull` and `/v1/pull/ack` are UNCHANGED for every route the invite system did not create** — one indexed lookup misses and the pre-existing path runs as before. **Two defects found by the D614 census are fixed here:** (a) the store's schema-version marker was written with `INSERT OR IGNORE`, a no-op on an existing key, so a forward migration never advanced it and D-0011's fail-closed downgrade guard had been inert since the moment it was written — the migration now advances it, with a positive AND negative control; (b) `tests/na0642_durability_restart.rs` was cited as the proof that "a 200 means fsynced" and cannot be — SIGKILL destroys a process, not the page cache, and that suite passes 3/3 with `synchronous=OFF`. Its header comment is corrected and **the test is kept unchanged** for the process-crash durability it genuinely proves; the fsync claim is discharged instead by `tests/na0678_invite_durability.rs`, which counts real fsync syscalls, asserts the fsync precedes the 200 on the wire, and **skips with a stated reason** when `strace` is absent rather than passing silently.
  - **Rationale:** The messaging epic's dependency chain requires the relay to expose invite and mailbox primitives before any client can redeem or handshake, and the epic's own scope split makes that ordering the safety property rather than a convenience. Building the invite ingress as new surface — rather than retro-gating the existing mailbox as the lane intent first proposed — is what keeps the shipped qsc client, the spine's pinned in-process e2e, the qsl-attachments interop contract and the live relay working while the client-side slices are still unwritten; retro-gating would have inverted the dependency chain it was meant to serve. Client-side capability minting was ruled after the census found the program authority and the design document specifying different parties: the design's relay-minting sentence was ruled a Director error predating the settled commitment architecture, and client-minting is strictly stronger at no cost. Wire surface is a recorded-decision area per D-0009/D-0010/D-0011/D-0012, so the addition is recorded here; governance authority lives in qsl-protocol.
  - **References:** qsl-protocol NA-0678 / QSL-DIR-2026-07-26-614 (D614) / D-1310, D-1311; `DESIGN_invite_system_v1.md` (operator-ratified, §3 corrected 2026-07-26); D-0011 (the durable store and its downgrade guard); D-0012 (the capability document); D-0014 (`ct_eq_secret`); `src/lib.rs`; `src/store.rs`; `src/main.rs`; `docs/server/DOC-SRV-007_Invite_Slot_Contract_v1.0.0_DRAFT.md`; `tests/na0678_invite_slots.rs`; `tests/na0678_schema_version.rs`; `tests/na0678_invite_durability.rs`; `tests/na0642_durability_restart.rs` (header comment only); `tests/na0652_server_info.rs` (the two exact guards); `README.md`; `packaging/systemd/relay.env.example`; `TRACEABILITY.md`

- **ID:** D-0017
  - **Status:** Accepted
  - **Date:** 2026-07-29
  - **Goals:** G4
  - **Decision:** Every log-capture assertion site in this repository **synchronises on the relay having emitted before it reads the capture buffer**, through one shared helper (`tests/common/mod.rs`: `capture()`, `await_log`, `await_logs`, `try_await_log`, `LogWaitError::Timeout`). **TEST-ONLY: no runtime code, no route, no handler, no store, no `Cargo.*`, no workflow, no `scripts/`.** Governed by qsl-protocol NA-0687 / QSL-DIR-2026-07-29-621 (D621) / D-1326, result class `LOG_CAPTURE_SYNC_SWEEP_PASS_WITH_SECOND_MECHANISM_FILED`. **The census measured 12 sites, not the "at least three" the ledger recorded** — every capture site in the repo, reconciled across three independent mechanism-keyed searches (capture-writer definitions, subscriber installs, buffer reads), and all 12 classified FIX with **zero** already-synchronised and **zero** not-this-pattern. On expiry the wait fails with a NAMED error naming the needle, the wait and **the size of the buffer it examined** — 5 s deadline, 50 ms poll, both derived from this project's existing readiness idiom rather than invented. ⚠ **Not one assertion about redaction or log content was changed; only WHEN the buffer is read.** ⚠ **`abort()` ordering is load-bearing**: 10 of the 12 sites called `handle.abort()` and then read, and on a current-thread runtime `abort()` guarantees a not-yet-emitted line is never emitted, so the wait must precede the abort — a wait placed after it converts a flake into a deterministic timeout.
  - **What the controls proved, because a fix whose control cannot fire has fixed nothing:** the UNFIXED shape under a deliberately withheld gate goes **RED** with `assertion failed: text.contains(NEEDLE)`, exit 101 (control A — applied temporarily, captured, reverted, and **the revert proved byte-identical by sha256** rather than assumed); the gate itself demonstrably withholds and then reveals (control A′, so the control instrument is not itself vacuous); the fixed shape observes a line released late and **provably waited ≥150 ms** for it (control B); an unreleased gate produces the named timeout over an **empty** buffer, bounded, never hanging (control C); and a **populated** buffer that lacks the needle times out with its size reported distinctly (control C2) — because *nothing emitted at all* and *the wrong thing emitted* are different defects and must not be reported by the same words.
  - **Measured, with the expectation written before every run:** baseline `RUST_TEST_THREADS=2` **28 binaries / 129 passed / 0 failed / 0 ignored / exit 0**; post-fix at the same thread count **29 / 134 / 0 / 0 / exit 0**, an **exact match** to the prediction (129 + 4 controls + 1 helper-copy control, and nothing else moved). At full parallelism on 6 cores the pre-fix suite was **RED in 1 of 5 runs** — the first local reproduction of this flake, landing on **both** of ENG-0091's named instances in one run, both **positive** assertions, with **zero** negative assertions failing in any run (independently confirming ENG-0091's "the assertion is the positive one, every time"). Post-fix, **1 of 5 runs was still red, at one site instead of two.** ⚠ **NO CLAIM IS MADE THAT THE FULL-SUITE FLAKE RATE FELL: it was 1-in-5 before and 1-in-5 after.** What is claimed, and measured, is that **failing sites in the red run went 2 → 1** and that **the failure became diagnosable**. Five runs cannot establish a rate in either direction.
  - **⚠ THE SECOND MECHANISM, and the fix is what revealed it.** The surviving failure reports `LOG_SYNC_TIMEOUT: needle "channel_id=" not observed within 5027ms (buffer 0 bytes, 0 lines)`. **`0 bytes` after the full deadline falsifies slow-emit outright** — nothing was ever captured. One discriminating experiment, prediction written first, both arms confirmed: **1 of 20 whole-binary runs red, 0 of 20 with that test alone**, so the failure **requires sibling tests in the same process** and is inconsistent with a per-emit race. Filed as **qsl-protocol ENG-0094** with its hypothesis labelled as inference (global callsite `Interest` caching vs thread-local `set_default`); **deliberately not fixed here**, because the confirming experiment *is* the candidate fix and the remedy needs a harness design decision this lane was not authorised to make. **ENG-0065 is closed; ENG-0091 stays OPEN on ENG-0094.** ⚠ **The pre-fix instrument could not distinguish the two mechanisms** — both printed `assertion failed: ...contains("channel_id=")` — so ENG-0091's original data points may include this one, and there is no way to tell retrospectively.
  - **What was deliberately NOT changed, and why each is recorded rather than left to be rediscovered:** (a) **`tokio`'s `time` feature is not declared by this crate** — it arrives transitively via `axum 0.7.9` and `reqwest 0.12.28`, which is what lets `tokio::time::sleep` compile with no `Cargo.toml` change; the helper carries a comment naming the fact and the no-feature fallback (`Instant` + `yield_now()`, at the cost of spinning a core). (b) **Six of the twelve sites carried a single `tokio::task::yield_now().await` before the abort/read** — a nudge, not a synchronisation: it grants the server task exactly one scheduling opportunity and neither waits for nor detects the emit. They were removed where the deliberate wait replaced them, because leaving a weaker mechanism beside a real one leaves the next reader unable to tell which is load-bearing. ⚠ **Every observed failure — both pre-fix and post-fix — landed in the six UN-nudged sites**, which include both ENG-0091 instances and ENG-0065's original. (c) **Two sites read the buffer strictly (`String::from_utf8` panicking on invalid UTF-8) and now read lossily**, matching the ten-site majority; nothing asserts UTF-8 validity of the log, every needle is ASCII, and lossy is the more permissive direction, so it cannot make a `contains` assertion pass that would otherwise fail. An exhaustive nine-item side-effect inventory was written **before** the ten hand-rolled writers were replaced; this was the only behavioural difference in it.
  - **⚠ `tests::payload_not_logged` ACQUIRED ITS FIRST FAILURE MODE BY OPERATOR RULING (2026-07-29, D621 F6+R4), and this is recorded so a future red traces here rather than surprising anyone.** Before this lane its **only** assertion was a negative one read **after** `abort()`, so an empty buffer satisfied it: **it could not fail, and had presumably measured nothing since the day it was written.** It now awaits the relay's `push channel_id=` line first. The anchor is a **synchronisation precondition, not a new content claim** — 7 of the 12 swept sites already assert that needle positively, so the requirement is the tree's own vocabulary — but the consequence is real and intended: if the relay ever stops logging the push line, this test goes red. *A test that cannot fail is not a weaker instrument; it is not an instrument.*
  - **⚠ ONE HELPER FOR THE INTEGRATION TESTS, TWO COPIES IN TOTAL — and both single-definition mechanisms were measured to fail from inside an inline `mod tests`, verbatim:** (1) `#[path = "../tests/common/mod.rs"] mod common;` → `error: couldn't read src/tests/../tests/common/mod.rs`, because a `#[path]` on a module declared inside an **inline** module resolves relative to `<dir of this file>/<inline module name>/` — the **phantom** directory `src/tests/` — and since it does not exist the kernel cannot resolve `..` through it, so no relative path escapes it (`../../` fails identically); (2) `mod common { include!("../tests/common/mod.rs"); }` → `error: an inner attribute is not permitted in this context` plus `error[E0753]: expected outer doc comment` ×6, because the helper file's `#![allow(dead_code)]` and `//!` module docs are exactly what make it a proper module file. The directive's ruled fallback was therefore taken: the integration tests share one definition via `mod common;`, and `src/lib.rs`'s test module carries a second copy naming that file as the source of truth. ⚠ **The copy is NOT unguarded** — it carries its own control (`log_sync_timeout_is_named_and_reports_what_it_read`), so a drift into vacuity there fails a test rather than passing quietly. **A third option exists and was deliberately NOT taken:** a top-level `#[cfg(test)] #[path = "../tests/common/mod.rs"] mod test_common;` resolves correctly (`src/..` exists) and is still test-only, but it sits outside D621 §6's permission to touch `src/lib.rs` *inside `#[cfg(test)] mod tests` only*. An authorised fallback existed, so the scope line was not stretched; a later lane may revisit this if the duplication bites.
  - **Rationale:** The defect was costing merges non-deterministically, and a gate that fails at random teaches reviewers to disbelieve reds — which is more expensive than the lost minutes. Both directions of the mistake were real: a positive assertion that fails when the emit loses the race, and a negative assertion that passes **vacuously** when read before the buffer is populated, which can never fail and therefore never measured anything. Fixing only the observed direction would have left half the population still asserting nothing. Doing it as one uniform helper rather than twelve hand-rolled repairs is what made the pattern auditable and what made the second mechanism visible at all: a shared error message that names the size of the buffer it examined is why `0 bytes` could be distinguished from "the wrong lines", and that single line of diagnostic text is the difference between a finding and another rerun.
  - **References:** qsl-protocol NA-0687 / QSL-DIR-2026-07-29-621 (D621) / D-1326; ENG-0091 (open, on ENG-0094), ENG-0065 (closed), **ENG-0094** (new, the second mechanism), ENG-0092 (new, this repo's `cargo test -q`), ENG-0093 (new, the scanner's `__pycache__`); `tests/common/mod.rs` (new); `tests/na0687_log_sync_controls.rs` (new); `src/lib.rs` (`#[cfg(test)] mod tests` only — `payload_not_logged`, `logs_do_not_contain_raw_channel`, `overload_logs_are_safe_and_structured`, the helper copy and its control); `tests/abuse_rate_queue_logging.rs`; `tests/hardening_auth_reject_logging.rs`; `tests/idempotency_logging.rs`; `tests/na0349_end_to_end_integration_contract.rs`; `tests/na0598_exact_4mib_relay_logging.rs`; `tests/na0642_retention_logging.rs`; `tests/na0678_invite_slots.rs`; `tests/qsl_attachments_integration_contract.rs`; `tests/rate_global_cap_logging.rs`; `TRACEABILITY.md`; D-0016 (the invite-slot subsystem, whose opacity test is ENG-0091 instance 1); D-0015 (the infra-literal gate, run clean at base and after)
