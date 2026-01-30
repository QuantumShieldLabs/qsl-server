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
