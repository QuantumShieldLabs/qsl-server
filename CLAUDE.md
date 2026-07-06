# CLAUDE.md — qsl-server (thin pointer; governance is authoritative)

Goals: G4 (primary), supports G1–G5

This repository is a satellite of qsl-protocol. All directive, queue, and
decision authority lives in the qsl-protocol governance spine; work here
occurs only when a qsl-protocol NA directive explicitly authorizes it.

Read first: START_HERE.md, README.md, DECISIONS.md, TRACEABILITY.md,
NEXT_ACTIONS.md in this repo, then the qsl-protocol spine (START_HERE.md,
GOALS.md, AGENTS.md, CODEX_RULES.md, NEXT_ACTIONS.md,
docs/ops/DIRECTOR_OPERATIONS.md). Every rule addressed to "Codex" binds you.

Hard boundaries for this repo:
- qsl-server is transport/relay/control-plane ONLY. Plaintext exposure class must remain no. Never add message-content awareness, decryption, or key material handling.
- Fail-closed everywhere; reject unknown versions/IDs/flags, truncation,
  trailing bytes; no best-effort parsing.
- No dependency/lockfile/workflow mutation unless the active qsl-protocol
  NA lane explicitly authorizes it.
- Merge commits only; no squash/rebase/force-push/amend after PR creation.
- Publish class summaries only; raw private values remain proof-root-only.
- No public/production/security-completion claims.

Precedence: this file is a pointer only; the governance spine wins on any
conflict.
