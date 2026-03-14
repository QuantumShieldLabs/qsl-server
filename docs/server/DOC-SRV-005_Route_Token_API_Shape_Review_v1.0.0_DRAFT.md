# DOC-SRV-005 Route-Token API Shape Review v1.0.0 (Draft)

Status: Draft

Purpose:
- review the current route-token-in-URL relay API shape
- record the threat model and migration decision without changing runtime behavior

## Current compatibility shape

- `POST /v1/push/:channel`
- `GET /v1/pull/:channel?max=N`
- `:channel` currently carries the route token

This is the current deployed compatibility shape only. This document does not authorize a runtime change by itself.

## Grounded leakage surfaces

1. Reverse proxies and access logs
   - The current Caddy example already requires `/v1/*` log suppression because the URI path carries capability-like route tokens.
   - If upstream proxies, load balancers, or default web access logs capture request URIs, route tokens leak into operator-visible logs.

2. Shell history and copied command lines
   - Current operator verification flows use full `curl` commands with `/v1/pull/$CH` and `/v1/push/$CH`.
   - Even when the token is test-only, this normalizes a handling pattern where real route tokens can be copied into history, terminals, tickets, or chat.

3. Support bundles, screenshots, and tutorials
   - README, runbooks, and API docs present the route token as part of the operator-facing URL shape.
   - Support captures and tutorial screenshots tend to preserve the visible URL path.

4. Metrics and observability traces
   - HTTP request instrumentation often records route templates and, in weaker setups, raw paths.
   - Path-embedded capability identifiers create unnecessary pressure on every telemetry sink to sanitize correctly.

5. Browser and tooling history
   - Browser address bars, developer tools, API clients, bookmark stores, and request-history tools preserve request URLs by default more often than headers or request bodies.

## Decision

Decision: MIGRATE away from URL-embedded route tokens.

Rationale:
- Route tokens behave like capability-bearing secrets and should not live in the most widely propagated part of the request.
- The current deployment guidance already needs compensating controls (`/v1/*` log suppression), which is strong evidence that the shape is operator-hostile.
- KEEP would leave the safety burden on every proxy, log sink, shell session, screenshot, and support workflow instead of fixing the API boundary.

## Why KEEP was rejected

- KEEP would require ongoing compensating controls across reverse proxies, service logs, telemetry, and operator tooling.
- Those controls are fragile and easy to regress outside the application code path.
- The docs already show that the current shape is sensitive enough to require special handling, so documenting more warnings is not a sufficient end state.

## Direct implementation follow-on requirements

The direct follow-on item must preserve compatibility while moving the route token out of the URL path.

Compatibility requirements:
- No silent break for existing clients.
- Provide a staged compatibility window where the current path shape and the replacement mechanism can coexist, or an equally safe compatibility strategy with explicit cutover criteria.
- The follow-on must define how client and server negotiate or default during rollout.

Migration and rollout criteria:
- Define the replacement token carriage mechanism explicitly.
- Define server-side acceptance rules during the compatibility window.
- Define deprecation and removal criteria for the URL-embedded shape.

Log-safety requirements:
- No raw route tokens in access logs, metrics labels, traces, support bundles, or screenshots.
- Update deployment examples and verification guidance so operators no longer normalize token-bearing URLs in copied commands.

Operator-change requirements:
- Update reverse-proxy examples, runbooks, verification scripts, and troubleshooting guides to the new shape.
- Preserve secret-safe examples throughout the migration period.

Validation requirements:
- Add deterministic compatibility tests for legacy and migrated shapes.
- Prove that normal logging and operator workflows do not expose raw route tokens.

## Non-goals for this item

- No qsl-server runtime/API/auth/relay-semantic changes
- No qsl-protocol client implementation changes
- No migration rollout implementation
