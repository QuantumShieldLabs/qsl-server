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
- To be populated with PR links, merge SHAs, and scope guard outputs.
