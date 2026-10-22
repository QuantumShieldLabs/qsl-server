# QSL Server

**Mailbox transport and invitation endpoints for QSL development.**

The relay stores and forwards client-protected frames. It does not hold the clients' session keys or perform their ratchet operations. Its invitation endpoints participate in contact setup through capabilities and one-use tickets.

> [!WARNING]
> Research stage. Not independently audited or production-ready. Local resource controls do not establish safe public deployment.

## Trust and visibility

| The relay handles | The operator can observe |
|---|---|
| Queued encrypted frames and leased retrieval | Mailbox access, request timing, frame size and traffic volume |
| Invitation creation, redemption and revocation | Invitation operations and relay-visible request fields |
| Local resource, expiry and admission state | Its stored state and operational behavior |

End-to-end content encryption does not conceal all metadata. Self-hosting changes who operates the relay; it does not prevent traffic correlation or make the operator unable to observe queue activity. Inspect the client protocol's current claim boundary in [qsl-protocol](https://github.com/QuantumShieldLabs/qsl-protocol).

## Current API surface

| Surface | Role |
|---|---|
| `POST /v1/push` | Store a frame on a route |
| `GET /v1/pull?max=N&ack=lease` | Retrieve leased items |
| `POST /v1/pull/ack` | Acknowledge leased items |
| `GET /v1/server-info` | Inspect advertised configuration and authentication mode |
| `/v1/invite/create`, `/redeem`, `/revoke` | Manage invitation slots |

These are the published v1 interfaces. Proposed successor interfaces are separate work. `X-Msg-Id` is not an idempotency key; duplicate values may create separate queued items.

## Run locally

```bash
cargo run
```

The default bind is loopback, `127.0.0.1:8080`. Keep development experiments local and use synthetic data.

`RELAY_TOKEN` controls bearer authentication. If unset, bearer authentication is disabled; route-token checks remain, but anyone able to reach the port can attempt requests. Check `/v1/server-info` rather than assuming authentication is enabled.

See the [configuration guide](docs/CONFIGURATION.md) for the published defaults and interface details. Public exposure needs operational and security validation beyond the local checks; a successful local run is not deployment approval.

## Review and contribute

Begin with [START_HERE.md](START_HERE.md) and [CONTRIBUTING.md](CONTRIBUTING.md). Review admission, queue bounds, authentication, leases, expiry, restart and error paths together. Avoid capability-bearing URLs and sensitive logs.

Report sensitive findings through [SECURITY.md](SECURITY.md). For protocol/cryptographic reports, use [qsl-protocol](https://github.com/QuantumShieldLabs/qsl-protocol).

## Related components and license

[Protocol and core](https://github.com/QuantumShieldLabs/qsl-protocol) · [Desktop prototype](https://github.com/QuantumShieldLabs/qsl-desktop) · [Encrypted-object service](https://github.com/QuantumShieldLabs/qsl-attachments)

`AGPL-3.0-only`. See [LICENSE](LICENSE).
