# DOC-SRV-005 Route-Token API Shape Review v1.0.0 (Draft)

Status: Draft

Purpose:
- review the former route-token-in-URL relay API shape
- record the threat model, migration decision, and current retired-route contract

## Canonical shape after migration

- `POST /v1/push`
- `GET /v1/pull?max=N`
- `X-QSL-Route-Token: <token>`

## Retired legacy shape

- `POST /v1/push/:channel`
- `GET /v1/pull/:channel?max=N`
- `:channel` carried the route token before retirement

The header-based shape is canonical. The path-based shape is retired in current code and must not be reintroduced without a separate, explicit semantic decision.

## Grounded leakage surfaces

1. Reverse proxies and access logs
   - Prior Caddy examples required `/v1/*` log suppression because the URI path carried capability-like route tokens.
   - If upstream proxies, load balancers, or default web access logs capture request URIs, route tokens leak into operator-visible logs.

2. Shell history and copied command lines
   - Legacy operator verification flows used full `curl` commands with `/v1/pull/$CH` and `/v1/push/$CH`.
   - Even when the token was test-only, this normalized a handling pattern where real route tokens could be copied into history, terminals, tickets, or chat.

3. Support bundles, screenshots, and tutorials
   - Older README, runbooks, and API docs presented the route token as part of the operator-facing URL shape.
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

## Implemented retired-route rules

Current requirements:
- Canonical routes are token-free in the URL and require `X-QSL-Route-Token`.
- Legacy path-based routes are not registered by current code.

Server-side acceptance rules:
- canonical `/v1/push` and `/v1/pull` with missing/empty `X-QSL-Route-Token` reject deterministically with no mutation
- legacy path-based push requests return 404 and do not mutate queues
- legacy path-based pull requests return 404 and do not consume canonical queue items
- `Authorization: Bearer ...` remains reserved for relay auth and is unchanged by route-token migration

Migration and rollout state:
- qsc and operator verification flows use the header-based shape by default
- docs/runbooks must treat the path-based shape as retired, not compatibility-only
- any future compatibility restoration would be a service semantic change requiring a separate directive and tests

Log-safety requirements:
- No raw route tokens in access logs, metrics labels, traces, support bundles, or screenshots.
- Keep deployment examples and verification guidance from normalizing token-bearing URLs in copied commands.

Operator-change requirements:
- Update reverse-proxy examples, runbooks, verification scripts, and troubleshooting guides to the new shape.
- Preserve secret-safe examples throughout the migration period.

Validation requirements:
- Keep deterministic tests that prove canonical header-based requests work and retired legacy routes 404 without queue mutation.
- Prove that normal logging and operator workflows do not expose raw route tokens.

## Non-goals for the retired-route contract

- No auth semantic change
- No route-token overloading into `Authorization`
- No unrelated API redesign
