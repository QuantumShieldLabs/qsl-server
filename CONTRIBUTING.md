# Contributing

Goals: G1, G2, G3, G4, G5

Revision: 2026-10-13. Read [the project workflow](PROJECT_WORKFLOW.md).

Implement one connected scoped change; run relevant targeted local checks and preserve all required CI before human merge. Log every actual shortcut in the one HARDENING.md ledger with cut/risk/mandatory trigger. Sensitive interfaces are independently reviewed before dependent use. An already scoped task does not need a duplicate issue or new routine approval. Report exact source, tests and limitations; never publish credentials/private raw evidence.

## Local checks
The required `rust` check runs, in order: `bash scripts/ci/test_aws_update_and_verify.sh`, `bash scripts/ci/test_update_checksum.sh`, `cargo fmt --all -- --check`, `cargo test -q`, and `cargo clippy -q -- -D warnings`. Run the same commands locally before opening a PR.

## Code of conduct
Behavior expectations are defined in `CODE_OF_CONDUCT.md`.
