# Start here

Goals: G1, G2, G3, G4, G5

Revision: 2026-10-13. Read [the project workflow](PROJECT_WORKFLOW.md).

Follow PROJECT_WORKFLOW.md in full for all project work. The relay stores/forwards opaque encrypted payloads. It does not decrypt message content or hold session keys. Keep authentication/capability separation, bounded strict input and no secret/payload logging. Its invite control-plane contract remains explicit.

The managed current lane TASK is the single task authority; no repository-specific queue launches work. Manual, one task at a time, normal speed, Pushover USER_PAUSED. Preserve issued inputs and frozen snapshots; required CI, safety core and human merge authority remain.

Read README.md for component purpose, the active task/roadmap card for scope and relevant accepted contracts/source. Full historical decisions and testplan archives are read when needed for the actual claim, not mandatory startup corpus.
