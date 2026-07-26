use std::{env, net::SocketAddr, time::Duration};

use clap::Parser;
use qsl_server::{
    app, pull_lease_or_error, retention_ttl_or_error, AppState, InviteLimits, Limits,
    ResourceControls, ServerInfoCfg, StoreConfig, INVITE_CREATE_BURST_DEFAULT,
    INVITE_CREATE_REFILL_PER_SEC_DEFAULT, MAX_BODY_BYTES_CEILING, MAX_INVITE_BUNDLE_BYTES_DEFAULT,
    MAX_INVITE_EXPIRY_SECS_DEFAULT, MAX_INVITE_SLOTS_DEFAULT, MAX_PUSH_RATE_BURST_CEILING,
    MAX_QUEUE_DEPTH_CEILING, MAX_ROUTE_COUNT_CEILING, PULL_LEASE_SECS_DEFAULT,
    RETENTION_TTL_SECS_DEFAULT,
};
use tokio::net::TcpListener;
use tracing::info;

const RETENTION_SWEEP_INTERVAL_SECS: u64 = 60;

#[derive(Parser, Debug)]
#[command(name = "qsl-server", version)]
struct Cli {
    /// Bind address (env: BIND_ADDR, default: 127.0.0.1)
    #[arg(long)]
    bind: Option<String>,
    /// Listen port (env: PORT, default: 8080)
    #[arg(long)]
    port: Option<u16>,
    /// Max body bytes (env: MAX_BODY_BYTES, default: 1048576)
    #[arg(long)]
    max_body_bytes: Option<usize>,
    /// Max queue depth (env: MAX_QUEUE_DEPTH, default: 257)
    #[arg(long)]
    max_queue_depth: Option<usize>,
    /// Max live routes (env: MAX_ROUTE_COUNT, default: 256)
    #[arg(long)]
    max_route_count: Option<usize>,
    /// Per-route push burst before rate limiting (env: PUSH_RATE_BURST, default: 257)
    #[arg(long)]
    push_rate_burst: Option<usize>,
    /// Per-route push token refill per second; 0 disables refill (env: PUSH_RATE_REFILL_PER_SEC, default: 257)
    #[arg(long)]
    push_rate_refill_per_sec: Option<usize>,
    /// DEPRECATED (NA-0642): ignored; message lifetime is RETENTION_TTL_SECS
    #[arg(long)]
    route_idle_ttl_ms: Option<usize>,
    /// SQLite store file for the durable queue; ':memory:' for ephemeral runs (env: STORE_PATH, required)
    #[arg(long)]
    store_path: Option<String>,
    /// Undelivered-message retention in seconds (env: RETENTION_TTL_SECS, default: 604800)
    #[arg(long)]
    retention_ttl_secs: Option<usize>,
    /// Ack-mode pull lease (visibility timeout) in seconds (env: PULL_LEASE_SECS, default: 60)
    #[arg(long)]
    pull_lease_secs: Option<usize>,
    /// Max live invite slots (env: MAX_INVITE_SLOTS, default: 256)
    #[arg(long)]
    max_invite_slots: Option<usize>,
    /// Max bytes per stored invite bundle or signature (env: MAX_INVITE_BUNDLE_BYTES, default: 16384)
    #[arg(long)]
    max_invite_bundle_bytes: Option<usize>,
    /// Max invite lifetime in seconds (env: MAX_INVITE_EXPIRY_SECS, default: 259200)
    #[arg(long)]
    max_invite_expiry_secs: Option<usize>,
    /// Global invite-create burst before rate limiting (env: INVITE_CREATE_BURST, default: 32)
    #[arg(long)]
    invite_create_burst: Option<usize>,
    /// Global invite-create token refill per second; 0 disables refill (env: INVITE_CREATE_REFILL_PER_SEC, default: 1)
    #[arg(long)]
    invite_create_refill_per_sec: Option<usize>,
}

#[derive(Clone, Debug)]
struct EnvVals {
    bind: Option<String>,
    port: Option<u16>,
    max_body_bytes: Option<usize>,
    max_queue_depth: Option<usize>,
    max_route_count: Option<usize>,
    push_rate_burst: Option<usize>,
    push_rate_refill_per_sec: Option<usize>,
    // Deprecated: presence is warned about, the value is never parsed.
    route_idle_ttl_ms_present: bool,
    store_path: Option<String>,
    retention_ttl_secs: Option<usize>,
    pull_lease_secs: Option<usize>,
    max_invite_slots: Option<usize>,
    max_invite_bundle_bytes: Option<usize>,
    max_invite_expiry_secs: Option<usize>,
    invite_create_burst: Option<usize>,
    invite_create_refill_per_sec: Option<usize>,
}

impl EnvVals {
    fn from_env() -> Result<Self, String> {
        Ok(Self {
            bind: env_opt("BIND_ADDR"),
            port: env_u16("PORT")?,
            max_body_bytes: env_usize("MAX_BODY_BYTES")?,
            max_queue_depth: env_usize("MAX_QUEUE_DEPTH")?,
            max_route_count: env_usize("MAX_ROUTE_COUNT")?,
            push_rate_burst: env_usize("PUSH_RATE_BURST")?,
            push_rate_refill_per_sec: env_usize("PUSH_RATE_REFILL_PER_SEC")?,
            route_idle_ttl_ms_present: env_opt("ROUTE_IDLE_TTL_MS").is_some(),
            store_path: env_opt("STORE_PATH"),
            retention_ttl_secs: env_usize("RETENTION_TTL_SECS")?,
            pull_lease_secs: env_usize("PULL_LEASE_SECS")?,
            max_invite_slots: env_usize("MAX_INVITE_SLOTS")?,
            max_invite_bundle_bytes: env_usize("MAX_INVITE_BUNDLE_BYTES")?,
            max_invite_expiry_secs: env_usize("MAX_INVITE_EXPIRY_SECS")?,
            invite_create_burst: env_usize("INVITE_CREATE_BURST")?,
            invite_create_refill_per_sec: env_usize("INVITE_CREATE_REFILL_PER_SEC")?,
        })
    }
}

#[derive(Clone, Debug)]
struct Config {
    bind: String,
    port: u16,
    limits: Limits,
    controls: ResourceControls,
    store: StoreConfig,
    invites: InviteLimits,
    deprecated_route_idle_ttl: bool,
}

fn env_opt(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn env_u16(name: &str) -> Result<Option<u16>, String> {
    match env_opt(name) {
        Some(v) => match v.parse::<u16>() {
            Ok(parsed) => Ok(Some(parsed)),
            Err(_) => Err(format!("ERR_INVALID_ENV_{}", name)),
        },
        None => Ok(None),
    }
}

fn env_usize(name: &str) -> Result<Option<usize>, String> {
    match env_opt(name) {
        Some(v) => v
            .parse::<usize>()
            .map(Some)
            .map_err(|_| format!("ERR_INVALID_CONFIG_{name}")),
        None => Ok(None),
    }
}

fn resolve_config(cli: Cli, env: EnvVals) -> Result<Config, String> {
    let bind = cli
        .bind
        .or(env.bind)
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port = cli.port.or(env.port).unwrap_or(8080);
    let max_body_bytes = cli
        .max_body_bytes
        .or(env.max_body_bytes)
        .unwrap_or(MAX_BODY_BYTES_CEILING);
    let max_queue_depth = cli
        .max_queue_depth
        .or(env.max_queue_depth)
        .unwrap_or(MAX_QUEUE_DEPTH_CEILING);
    let max_route_count = cli
        .max_route_count
        .or(env.max_route_count)
        .unwrap_or(MAX_ROUTE_COUNT_CEILING);
    let push_rate_burst = cli
        .push_rate_burst
        .or(env.push_rate_burst)
        .unwrap_or(MAX_PUSH_RATE_BURST_CEILING);
    let push_rate_refill_per_sec = cli
        .push_rate_refill_per_sec
        .or(env.push_rate_refill_per_sec)
        .unwrap_or(MAX_PUSH_RATE_BURST_CEILING);
    // Fail-closed: the durable store location must be explicit. ':memory:'
    // is accepted only as a deliberate ephemeral choice.
    let store_path = cli
        .store_path
        .or(env.store_path)
        .ok_or_else(|| "ERR_INVALID_CONFIG_STORE_PATH".to_string())?;
    let retention_ttl_secs = retention_ttl_or_error(
        cli.retention_ttl_secs
            .or(env.retention_ttl_secs)
            .unwrap_or(RETENTION_TTL_SECS_DEFAULT),
    )?;
    let pull_lease_secs = pull_lease_or_error(
        cli.pull_lease_secs
            .or(env.pull_lease_secs)
            .unwrap_or(PULL_LEASE_SECS_DEFAULT),
    )?;
    let invites = InviteLimits::new(
        cli.max_invite_slots
            .or(env.max_invite_slots)
            .unwrap_or(MAX_INVITE_SLOTS_DEFAULT),
        cli.max_invite_bundle_bytes
            .or(env.max_invite_bundle_bytes)
            .unwrap_or(MAX_INVITE_BUNDLE_BYTES_DEFAULT),
        cli.max_invite_expiry_secs
            .or(env.max_invite_expiry_secs)
            .unwrap_or(MAX_INVITE_EXPIRY_SECS_DEFAULT),
        cli.invite_create_burst
            .or(env.invite_create_burst)
            .unwrap_or(INVITE_CREATE_BURST_DEFAULT),
        cli.invite_create_refill_per_sec
            .or(env.invite_create_refill_per_sec)
            .unwrap_or(INVITE_CREATE_REFILL_PER_SEC_DEFAULT),
    )?;
    let deprecated_route_idle_ttl =
        cli.route_idle_ttl_ms.is_some() || env.route_idle_ttl_ms_present;
    Ok(Config {
        bind,
        port,
        limits: Limits::new(max_body_bytes, max_queue_depth)?,
        controls: ResourceControls::new(
            max_route_count,
            push_rate_burst,
            push_rate_refill_per_sec,
        )?,
        store: StoreConfig {
            path: store_path,
            retention_ttl_secs,
            pull_lease_secs,
        },
        invites,
        deprecated_route_idle_ttl,
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let env_vals = match EnvVals::from_env() {
        Ok(v) => v,
        Err(code) => {
            tracing::error!("{code}");
            std::process::exit(1);
        }
    };
    let cfg = match resolve_config(cli, env_vals) {
        Ok(v) => v,
        Err(code) => {
            tracing::error!("{code}");
            std::process::exit(1);
        }
    };
    if cfg.deprecated_route_idle_ttl {
        tracing::warn!(
            "event=deprecated_config name=ROUTE_IDLE_TTL_MS action=ignored replacement=RETENTION_TTL_SECS"
        );
    }
    let addr: SocketAddr = match format!("{}:{}", cfg.bind, cfg.port).parse() {
        Ok(v) => v,
        Err(_) => {
            tracing::error!("ERR_BIND_PARSE");
            std::process::exit(1);
        }
    };
    let listener = match TcpListener::bind(addr).await {
        Ok(v) => v,
        Err(_) => {
            tracing::error!("ERR_BIND_LISTEN");
            std::process::exit(1);
        }
    };
    let addr = match listener.local_addr() {
        Ok(v) => v,
        Err(_) => {
            tracing::error!("ERR_BIND_LISTEN");
            std::process::exit(1);
        }
    };

    let state = match AppState::new_full(
        cfg.limits,
        cfg.controls,
        std::env::var("RELAY_TOKEN").ok().filter(|v| !v.is_empty()),
        cfg.store,
        ServerInfoCfg::from_env(),
        cfg.invites,
    ) {
        Ok(v) => v,
        Err(code) => {
            tracing::error!("{code}");
            std::process::exit(1);
        }
    };
    let sweeper_state = state.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(RETENTION_SWEEP_INTERVAL_SECS));
        ticker.tick().await; // first tick fires immediately; skip it
        loop {
            ticker.tick().await;
            sweeper_state.run_retention_sweep();
        }
    });
    let app = app(state);

    info!("qsl-server listening on {}", addr);
    if axum::serve(listener, app).await.is_err() {
        tracing::error!("ERR_SERVER_RUNTIME");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    use qsl_server::{MAX_PULL_LEASE_SECS_CEILING, MAX_RETENTION_TTL_SECS_CEILING};

    fn base_cli() -> Cli {
        Cli {
            bind: None,
            port: None,
            max_body_bytes: None,
            max_queue_depth: None,
            max_route_count: None,
            push_rate_burst: None,
            push_rate_refill_per_sec: None,
            route_idle_ttl_ms: None,
            store_path: Some(":memory:".to_string()),
            retention_ttl_secs: None,
            pull_lease_secs: None,
            max_invite_slots: None,
            max_invite_bundle_bytes: None,
            max_invite_expiry_secs: None,
            invite_create_burst: None,
            invite_create_refill_per_sec: None,
        }
    }

    fn base_env() -> EnvVals {
        EnvVals {
            bind: None,
            port: None,
            max_body_bytes: None,
            max_queue_depth: None,
            max_route_count: None,
            push_rate_burst: None,
            push_rate_refill_per_sec: None,
            route_idle_ttl_ms_present: false,
            store_path: None,
            retention_ttl_secs: None,
            pull_lease_secs: None,
            max_invite_slots: None,
            max_invite_bundle_bytes: None,
            max_invite_expiry_secs: None,
            invite_create_burst: None,
            invite_create_refill_per_sec: None,
        }
    }

    #[test]
    fn cli_overrides_env() {
        let mut cli = base_cli();
        cli.port = Some(9000);
        cli.max_body_bytes = Some(4096);
        cli.max_queue_depth = Some(9);
        cli.max_route_count = Some(8);
        cli.push_rate_burst = Some(7);
        cli.push_rate_refill_per_sec = Some(6);
        cli.store_path = Some("/tmp/cli.db".to_string());
        cli.retention_ttl_secs = Some(120);
        cli.pull_lease_secs = Some(30);
        let mut env = base_env();
        env.bind = Some("0.0.0.0".to_string());
        env.port = Some(8080);
        env.max_body_bytes = Some(1024);
        env.max_queue_depth = Some(1);
        env.max_route_count = Some(2);
        env.push_rate_burst = Some(3);
        env.push_rate_refill_per_sec = Some(4);
        env.store_path = Some("/tmp/env.db".to_string());
        env.retention_ttl_secs = Some(999);
        env.pull_lease_secs = Some(99);
        let cfg = resolve_config(cli, env).unwrap();
        assert_eq!(cfg.bind, "0.0.0.0");
        assert_eq!(cfg.port, 9000);
        assert_eq!(cfg.limits.max_body_bytes, 4096);
        assert_eq!(cfg.limits.max_queue_depth, 9);
        assert_eq!(cfg.controls.max_route_count, 8);
        assert_eq!(cfg.controls.push_rate_burst, 7);
        assert_eq!(cfg.controls.push_rate_refill_per_sec, 6);
        assert_eq!(cfg.store.path, "/tmp/cli.db");
        assert_eq!(cfg.store.retention_ttl_secs, 120);
        assert_eq!(cfg.store.pull_lease_secs, 30);
    }

    #[test]
    fn env_overrides_defaults() {
        let cli = Cli {
            store_path: None,
            ..base_cli()
        };
        let mut env = base_env();
        env.bind = Some("0.0.0.0".to_string());
        env.port = Some(7070);
        env.max_body_bytes = Some(2048);
        env.max_queue_depth = Some(7);
        env.max_route_count = Some(6);
        env.push_rate_burst = Some(5);
        env.push_rate_refill_per_sec = Some(4);
        env.store_path = Some("/tmp/env-only.db".to_string());
        env.retention_ttl_secs = Some(3600);
        env.pull_lease_secs = Some(120);
        let cfg = resolve_config(cli, env).unwrap();
        assert_eq!(cfg.bind, "0.0.0.0");
        assert_eq!(cfg.port, 7070);
        assert_eq!(cfg.limits.max_body_bytes, 2048);
        assert_eq!(cfg.limits.max_queue_depth, 7);
        assert_eq!(cfg.controls.max_route_count, 6);
        assert_eq!(cfg.controls.push_rate_burst, 5);
        assert_eq!(cfg.controls.push_rate_refill_per_sec, 4);
        assert_eq!(cfg.store.path, "/tmp/env-only.db");
        assert_eq!(cfg.store.retention_ttl_secs, 3600);
        assert_eq!(cfg.store.pull_lease_secs, 120);
    }

    #[test]
    fn store_defaults_apply_when_unset() {
        let cfg = resolve_config(base_cli(), base_env()).unwrap();
        assert_eq!(cfg.store.path, ":memory:");
        assert_eq!(cfg.store.retention_ttl_secs, RETENTION_TTL_SECS_DEFAULT);
        assert_eq!(cfg.store.pull_lease_secs, PULL_LEASE_SECS_DEFAULT);
        assert!(!cfg.deprecated_route_idle_ttl);
    }

    #[test]
    fn missing_store_path_fails_closed() {
        let cli = Cli {
            store_path: None,
            ..base_cli()
        };
        let err = resolve_config(cli, base_env()).unwrap_err();
        assert_eq!(err, "ERR_INVALID_CONFIG_STORE_PATH");
    }

    #[test]
    fn zero_retention_or_lease_fails_closed() {
        let mut cli = base_cli();
        cli.retention_ttl_secs = Some(0);
        assert_eq!(
            resolve_config(cli, base_env()).unwrap_err(),
            "ERR_INVALID_CONFIG_RETENTION_TTL_SECS"
        );
        let mut cli = base_cli();
        cli.pull_lease_secs = Some(0);
        assert_eq!(
            resolve_config(cli, base_env()).unwrap_err(),
            "ERR_INVALID_CONFIG_PULL_LEASE_SECS"
        );
    }

    #[test]
    fn retention_and_lease_are_capped() {
        let mut cli = base_cli();
        cli.retention_ttl_secs = Some(MAX_RETENTION_TTL_SECS_CEILING * 2);
        cli.pull_lease_secs = Some(MAX_PULL_LEASE_SECS_CEILING * 2);
        let cfg = resolve_config(cli, base_env()).unwrap();
        assert_eq!(cfg.store.retention_ttl_secs, MAX_RETENTION_TTL_SECS_CEILING);
        assert_eq!(cfg.store.pull_lease_secs, MAX_PULL_LEASE_SECS_CEILING);
    }

    #[test]
    fn deprecated_route_idle_ttl_is_flagged_not_fatal() {
        let mut cli = base_cli();
        cli.route_idle_ttl_ms = Some(5_000);
        let cfg = resolve_config(cli, base_env()).unwrap();
        assert!(cfg.deprecated_route_idle_ttl);

        let mut env = base_env();
        env.route_idle_ttl_ms_present = true;
        let cfg = resolve_config(base_cli(), env).unwrap();
        assert!(cfg.deprecated_route_idle_ttl);
    }

    #[test]
    fn limits_are_capped() {
        let mut cli = base_cli();
        cli.max_body_bytes = Some(MAX_BODY_BYTES_CEILING * 2);
        cli.max_queue_depth = Some(MAX_QUEUE_DEPTH_CEILING * 2);
        cli.max_route_count = Some(MAX_ROUTE_COUNT_CEILING * 2);
        cli.push_rate_burst = Some(MAX_PUSH_RATE_BURST_CEILING * 2);
        cli.push_rate_refill_per_sec = Some(MAX_PUSH_RATE_BURST_CEILING * 20);
        let cfg = resolve_config(cli, base_env()).unwrap();
        assert_eq!(cfg.limits.max_body_bytes, MAX_BODY_BYTES_CEILING);
        assert_eq!(cfg.limits.max_queue_depth, MAX_QUEUE_DEPTH_CEILING);
        assert_eq!(cfg.controls.max_route_count, MAX_ROUTE_COUNT_CEILING);
        assert_eq!(cfg.controls.push_rate_burst, MAX_PUSH_RATE_BURST_CEILING);
        assert_eq!(
            cfg.controls.push_rate_refill_per_sec,
            qsl_server::MAX_PUSH_RATE_REFILL_PER_SEC_CEILING
        );
    }

    #[test]
    fn default_bind_is_loopback() {
        let cfg = resolve_config(base_cli(), base_env()).unwrap();
        assert_eq!(cfg.bind, "127.0.0.1");
    }

    #[test]
    fn explicit_public_bind_is_opt_in() {
        let mut cli = base_cli();
        cli.bind = Some("0.0.0.0".to_string());
        let cfg = resolve_config(cli, base_env()).unwrap();
        assert_eq!(cfg.bind, "0.0.0.0");
    }

    #[test]
    fn env_bind_can_enable_public_bind() {
        let mut env = base_env();
        env.bind = Some("0.0.0.0".to_string());
        let cfg = resolve_config(base_cli(), env).unwrap();
        assert_eq!(cfg.bind, "0.0.0.0");
    }

    #[test]
    fn invalid_port_env_is_rejected() {
        // Strict parse is deterministic; invalid values never silently fall back.
        std::env::set_var("PORT_INVALID_FOR_TEST", "not-a-port");
        let result = env_u16("PORT_INVALID_FOR_TEST");
        std::env::remove_var("PORT_INVALID_FOR_TEST");
        assert_eq!(result.unwrap_err(), "ERR_INVALID_ENV_PORT_INVALID_FOR_TEST");
    }

    #[test]
    fn zero_limits_are_rejected() {
        let mut env = base_env();
        env.max_body_bytes = Some(0);
        env.max_queue_depth = Some(1);
        let body_err = resolve_config(base_cli(), env).unwrap_err();
        assert_eq!(body_err, "ERR_INVALID_CONFIG_MAX_BODY_BYTES");

        let mut env = base_env();
        env.max_body_bytes = Some(1);
        env.max_queue_depth = Some(0);
        let depth_err = resolve_config(base_cli(), env).unwrap_err();
        assert_eq!(depth_err, "ERR_INVALID_CONFIG_MAX_QUEUE_DEPTH");
    }

    #[test]
    fn invalid_size_env_is_rejected() {
        std::env::set_var("MAX_BODY_BYTES_INVALID_FOR_TEST", "not-a-size");
        let result = env_usize("MAX_BODY_BYTES_INVALID_FOR_TEST");
        std::env::remove_var("MAX_BODY_BYTES_INVALID_FOR_TEST");
        assert_eq!(
            result.unwrap_err(),
            "ERR_INVALID_CONFIG_MAX_BODY_BYTES_INVALID_FOR_TEST"
        );
    }
}
