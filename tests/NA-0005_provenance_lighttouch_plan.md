# NA-0005 — qsl-server Provenance Light Touch Plan

Status: READY

Scope:
- Governance docs: `NEXT_ACTIONS.md`, `TRACEABILITY.md`, `DECISIONS.md`.
- Root docs: `NOTICE`, `PROVENANCE.md`, `SIGNED_RELEASES_RUNBOOK.md`.
- No server code, systemd, scripts, or workflow changes.

Objectives:
1. Define official-source provenance guidance for qsl-server consumers/operators.
2. Provide reproducible verification steps (commit SHA, signed tag, checksum).
3. Keep claims aligned with current server auth/logging posture.

Implementation steps:
1. Promote NA-0005 to READY via governance PR.
2. Add root provenance docs.
3. Add TRACEABILITY implementation line with PR link.
4. Close out NA-0005 and restore READY=0.

Verification checklist:
- `rg -n "Status:\\s*READY" NEXT_ACTIONS.md`:
  - governance phase: exactly one READY (`NA-0005`)
  - final phase: zero READY
- PR scope guard (`gh pr diff <PR> --name-only`) only includes allowed files.
- Checks green (or explicitly captured "no checks reported" before merge decision).

Executed evidence:
- Governance READY merged: PR #15 (https://github.com/QuantumShieldLabs/qsl-server/pull/15), merge SHA `8ad098c191ba7e8ca6e2296067304282f6225d1b`.
- Added root docs:
  - `NOTICE`
  - `PROVENANCE.md`
  - `SIGNED_RELEASES_RUNBOOK.md`
- Link/auth/runbook inventory command:
  - `rg -n "QuantumShieldLabs/qsl-server|QuantumShieldLabs/qsl-protocol|Authorization: Bearer|RELAY_TOKEN|sha256sum|git tag -s|git tag -v" NOTICE PROVENANCE.md SIGNED_RELEASES_RUNBOOK.md`
