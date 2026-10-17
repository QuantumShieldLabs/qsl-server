# Contributing

Goals: G1, G2, G3, G4, G5

Revision: 2026-10-13. Read [the project workflow](PROJECT_WORKFLOW.md).

Follow PROJECT_WORKFLOW.md for task scope, safety, independent review and hardening gates. This guide adds component-local check references and contributor conduct.

## Local checks
The required `rust` check runs, in order: `bash scripts/ci/test_aws_update_and_verify.sh`, `bash scripts/ci/test_update_checksum.sh`, `cargo fmt --all -- --check`, `cargo test -q`, and `cargo clippy -q -- -D warnings`. Choose local checks for the changed boundary under the exact task/card plan. Documentation-only changes use relevant documentation/contract and publication checks. Required CI still runs in full and must pass before merge; task/card-required local workloads remain binding.

## Code of conduct
Behavior expectations are defined in `CODE_OF_CONDUCT.md`.
