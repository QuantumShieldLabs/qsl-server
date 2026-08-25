# Contributing

## Scope
This repository is a satellite of the QSL protocol project. Governance, planning, and decisions originate in the qsl-protocol repository; changes here land through lanes governed by that spine. Protocol-level work (specifications, cryptography, the qsc client) belongs in qsl-protocol.

## Workflow
1) Open an issue describing the change before any PR.
2) Keep changes minimal and scoped; avoid unrelated refactors.
3) Every PR must pass the three required CI checks: `rust`, `advisories`, and `infra-literal-scan`.
4) Merge commits only (no squash or rebase merges).

## Local checks
The required `rust` check runs, in order: `bash scripts/ci/test_aws_update_and_verify.sh`, `bash scripts/ci/test_update_checksum.sh`, `cargo fmt --all -- --check`, `cargo test -q`, and `cargo clippy -q -- -D warnings`. Run the same commands locally before opening a PR.

## Code of conduct
Behavior expectations are defined in `CODE_OF_CONDUCT.md`.
