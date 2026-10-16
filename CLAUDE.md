# Assistant entry

Goals: G1, G2, G3, G4, G5

Read [PROJECT_WORKFLOW.md](PROJECT_WORKFLOW.md) in full for current process. The managed live TASK supplies current assignment and mode; this repository has no independent launch queue.

The relay stores/forwards opaque encrypted payloads. It does not decrypt message content or hold session keys. Keep authentication/capability separation, bounded strict input and no secret/payload logging. Its invite control-plane contract remains explicit.
