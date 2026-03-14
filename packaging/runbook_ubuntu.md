# QSL Relay Production Runbook (Ubuntu)

This runbook covers install, update, rollback, token rotation, and verification for `qsl-server`.

## 1) Prerequisites
- Ubuntu host with `systemd`.
- Public ingress should be terminated at reverse proxy (Caddy) on 443 (and optional 80 redirect).
- Keep relay app port `8080` private; do not expose it publicly.

## 2) Build and install
On the build host or target host:

```bash
cd /path/to/qsl-server
cargo build --release
```

Install on the target host as root:

```bash
cd /path/to/qsl-server
sudo bash scripts/install_ubuntu.sh ./target/release/qsl-server
```

Installed artifacts:
- Binary: `/opt/qsl-server/bin/qsl-server`
- Service: `/etc/systemd/system/qsl-server.service`
- Env file: `/etc/qsl-server/relay.env`
- Deployment metadata: `/opt/qsl-server/DEPLOYMENT_INFO`

## 3) Configure relay env
Edit `/etc/qsl-server/relay.env`:

- `BIND_ADDR=127.0.0.1` (default safe bind)
- `PORT=8080`
- `RELAY_TOKEN=` (set a strong token out-of-band; do not commit tokens)
- `MAX_BODY_BYTES`, `MAX_QUEUE_DEPTH` as needed

Apply config:

```bash
sudo systemctl daemon-reload
sudo systemctl restart qsl-server
sudo systemctl status qsl-server --no-pager
```

## 4) Configure Caddy reverse proxy
Use `packaging/caddy/Caddyfile.example` as baseline.

- Route public traffic to `127.0.0.1:8080`.
- Keep `/v1/*` access logging disabled/sanitized during the compatibility window. Canonical clients use `X-QSL-Route-Token`, but legacy path-based clients may still place route tokens in request URIs until explicit removal.

Restart Caddy after config changes.

## 5) Update flow (recommended: one-command wrapper with backup + verify)
One-command update from a release tag with preflight, backup, checksum verify, and post-checks:

```bash
cd /path/to/qsl-server
sudo bash scripts/aws_update_and_verify.sh --release v0.0.3 --repo QuantumShieldLabs/qsl-server --base-dir /opt/qsl-server
```

Marker meanings:
- `QSL_AWS_UPDATE_STEP=<name> status=ok|fail` tracks each deterministic phase.
- `QSL_AWS_UPDATE_RESULT PASS|FAIL code=<reason>` is the final outcome.

Backups are stored under `/root/qsl-backups/<UTC_TIMESTAMP>` by default (override with `--backup-dir`).
The updater preserves the canonical unit/env layout already installed on the host and refreshes `/opt/qsl-server/DEPLOYMENT_INFO`.

Fallback (build-from-source on host):

```bash
cd /path/to/qsl-server
cargo build --release
sudo bash scripts/update_ubuntu.sh ./target/release/qsl-server
```

Verification:
```bash
sudo bash scripts/verify_remote.sh
sudo systemctl status qsl-server --no-pager
sudo ss -lntp | rg ':8080|:443|:80'
```

## 6) Rollback
If a release fails, use the latest backup copy and restart:

```bash
sudo cp /root/qsl-backups/<UTC_TIMESTAMP>/qsl-server.bin /opt/qsl-server/bin/qsl-server
sudo systemctl restart qsl-server
sudo systemctl status qsl-server --no-pager
```

## 7) Token rotation checklist
1. Generate a new token outside the repo and outside shell history where possible.
2. Update `/etc/qsl-server/relay.env` with new `RELAY_TOKEN`.
3. Restart server: `sudo systemctl restart qsl-server`.
4. Update clients (QSC): `/relay set token <new-token>`.
5. Validate new token works and old token is rejected.

## 8) Firewall / Security Group guidance
- Public: `443` (and optional `80` redirect).
- Private: `8080` should not be publicly reachable.

## 9) Verification commands (copy/paste)
```bash
# listener footprint
sudo ss -lntp | rg ':80|:443|:8080'

# service status
sudo systemctl status qsl-server --no-pager

# local unauthorized check (without Authorization header)
curl -i -H "X-QSL-Route-Token: audit" "http://127.0.0.1:8080/v1/pull?max=1"

# confirm loopback bind unless explicitly opted in
sudo ss -lntp | rg ':8080'
```

## 10) Audit script
Run the included audit script for a consolidated report:

```bash
sudo bash scripts/qsl_relay_audit.sh
```
