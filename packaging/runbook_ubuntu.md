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
- Ensure `/v1/*` access logging is disabled/sanitized to avoid route-token leakage in URI paths.

Restart Caddy after config changes.

## 5) Update flow (recommended: checksum-verified release artifact)
One-command update from a signed GitHub release tag:

```bash
cd /path/to/qsl-server
sudo bash scripts/update_from_release.sh --release vX.Y.Z
```

This path is fail-closed:
- downloads `qsl-server-linux-x86_64` and `qsl-server-linux-x86_64.sha256`
- verifies SHA256 before any service stop/replacement
- applies atomic install with rollback on start failure

Fallback (build-from-source on host):

```bash
cd /path/to/qsl-server
cargo build --release
sudo bash scripts/update_ubuntu.sh ./target/release/qsl-server
```

Verification:
```bash
sudo systemctl status qsl-server --no-pager
sudo ss -lntp | rg ':8080|:443|:80'
```

## 6) Rollback
If a release fails, redeploy the previous known-good binary and restart:

```bash
sudo cp /path/to/previous/qsl-server /opt/qsl-server/bin/qsl-server
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
curl -i "http://127.0.0.1:8080/v1/pull/audit?max=1"

# confirm loopback bind unless explicitly opted in
sudo ss -lntp | rg ':8080'
```

## 10) Audit script
Run the included audit script for a consolidated report:

```bash
sudo bash scripts/qsl_relay_audit.sh
```
