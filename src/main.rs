use std::{env, net::SocketAddr};

use clap::Parser;
use qsl_server::{
    app, AppState, Limits, ResourceControls, MAX_BODY_BYTES_CEILING, MAX_PUSH_RATE_BURST_CEILING,
    MAX_QUEUE_DEPTH_CEILING, MAX_ROUTE_COUNT_CEILING, ROUTE_IDLE_TTL_MS_DEFAULT,
};
use tokio::net::TcpListener;
use tracing::info;

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
    /// Route idle TTL in milliseconds (env: ROUTE_IDLE_TTL_MS, default: 300000)
    #[arg(long)]
    route_idle_ttl_ms: Option<usize>,
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
    route_idle_ttl_ms: Option<usize>,
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
            route_idle_ttl_ms: env_usize("ROUTE_IDLE_TTL_MS")?,
        })
    }
}

#[derive(Clone, Debug)]
struct Config {
    bind: String,
    port: u16,
    limits: Limits,
    controls: ResourceControls,
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
    let route_idle_ttl_ms = cli
        .route_idle_ttl_ms
        .or(env.route_idle_ttl_ms)
        .unwrap_or(ROUTE_IDLE_TTL_MS_DEFAULT);
    Ok(Config {
        bind,
        port,
        limits: Limits::new(max_body_bytes, max_queue_depth)?,
        controls: ResourceControls::new_with_route_idle_ttl_ms(
            max_route_count,
            push_rate_burst,
            push_rate_refill_per_sec,
            route_idle_ttl_ms,
        )?,
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

    let state = AppState::new_with_controls(cfg.limits, cfg.controls);
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

    fn env_vals(
        bind: Option<&str>,
        port: Option<u16>,
        max_body_bytes: Option<usize>,
        max_queue_depth: Option<usize>,
    ) -> EnvVals {
        EnvVals {
            bind: bind.map(|v| v.to_string()),
            port,
            max_body_bytes,
            max_queue_depth,
            max_route_count: None,
            push_rate_burst: None,
            push_rate_refill_per_sec: None,
            route_idle_ttl_ms: None,
        }
    }

    #[test]
    fn cli_overrides_env() {
        let cli = Cli {
            bind: None,
            port: Some(9000),
            max_body_bytes: Some(4096),
            max_queue_depth: Some(9),
            max_route_count: Some(8),
            push_rate_burst: Some(7),
            push_rate_refill_per_sec: Some(6),
            route_idle_ttl_ms: Some(5_000),
        };
        let mut env = env_vals(Some("0.0.0.0"), Some(8080), Some(1024), Some(1));
        env.max_route_count = Some(2);
        env.push_rate_burst = Some(3);
        env.push_rate_refill_per_sec = Some(4);
        env.route_idle_ttl_ms = Some(3_000);
        let cfg = resolve_config(cli, env).unwrap();
        assert_eq!(cfg.bind, "0.0.0.0");
        assert_eq!(cfg.port, 9000);
        assert_eq!(cfg.limits.max_body_bytes, 4096);
        assert_eq!(cfg.limits.max_queue_depth, 9);
        assert_eq!(cfg.controls.max_route_count, 8);
        assert_eq!(cfg.controls.push_rate_burst, 7);
        assert_eq!(cfg.controls.push_rate_refill_per_sec, 6);
        assert_eq!(cfg.controls.route_idle_ttl.as_millis(), 5_000);
    }

    #[test]
    fn env_overrides_defaults() {
        let cli = Cli {
            bind: None,
            port: None,
            max_body_bytes: None,
            max_queue_depth: None,
            max_route_count: None,
            push_rate_burst: None,
            push_rate_refill_per_sec: None,
            route_idle_ttl_ms: None,
        };
        let mut env = env_vals(Some("0.0.0.0"), Some(7070), Some(2048), Some(7));
        env.max_route_count = Some(6);
        env.push_rate_burst = Some(5);
        env.push_rate_refill_per_sec = Some(4);
        env.route_idle_ttl_ms = Some(3_000);
        let cfg = resolve_config(cli, env).unwrap();
        assert_eq!(cfg.bind, "0.0.0.0");
        assert_eq!(cfg.port, 7070);
        assert_eq!(cfg.limits.max_body_bytes, 2048);
        assert_eq!(cfg.limits.max_queue_depth, 7);
        assert_eq!(cfg.controls.max_route_count, 6);
        assert_eq!(cfg.controls.push_rate_burst, 5);
        assert_eq!(cfg.controls.push_rate_refill_per_sec, 4);
        assert_eq!(cfg.controls.route_idle_ttl.as_millis(), 3_000);
    }

    #[test]
    fn limits_are_capped() {
        let cli = Cli {
            bind: None,
            port: None,
            max_body_bytes: Some(MAX_BODY_BYTES_CEILING * 2),
            max_queue_depth: Some(MAX_QUEUE_DEPTH_CEILING * 2),
            max_route_count: Some(MAX_ROUTE_COUNT_CEILING * 2),
            push_rate_burst: Some(MAX_PUSH_RATE_BURST_CEILING * 2),
            push_rate_refill_per_sec: Some(MAX_PUSH_RATE_BURST_CEILING * 20),
            route_idle_ttl_ms: Some(qsl_server::MAX_ROUTE_IDLE_TTL_MS_CEILING * 2),
        };
        let env = env_vals(None, None, None, None);
        let cfg = resolve_config(cli, env).unwrap();
        assert_eq!(cfg.limits.max_body_bytes, MAX_BODY_BYTES_CEILING);
        assert_eq!(cfg.limits.max_queue_depth, MAX_QUEUE_DEPTH_CEILING);
        assert_eq!(cfg.controls.max_route_count, MAX_ROUTE_COUNT_CEILING);
        assert_eq!(cfg.controls.push_rate_burst, MAX_PUSH_RATE_BURST_CEILING);
        assert_eq!(
            cfg.controls.push_rate_refill_per_sec,
            qsl_server::MAX_PUSH_RATE_REFILL_PER_SEC_CEILING
        );
        assert_eq!(
            cfg.controls.route_idle_ttl.as_millis(),
            qsl_server::MAX_ROUTE_IDLE_TTL_MS_CEILING as u128
        );
    }

    #[test]
    fn default_bind_is_loopback() {
        let cli = Cli {
            bind: None,
            port: None,
            max_body_bytes: None,
            max_queue_depth: None,
            max_route_count: None,
            push_rate_burst: None,
            push_rate_refill_per_sec: None,
            route_idle_ttl_ms: None,
        };
        let env = env_vals(None, None, None, None);
        let cfg = resolve_config(cli, env).unwrap();
        assert_eq!(cfg.bind, "127.0.0.1");
    }

    #[test]
    fn explicit_public_bind_is_opt_in() {
        let cli = Cli {
            bind: Some("0.0.0.0".to_string()),
            port: None,
            max_body_bytes: None,
            max_queue_depth: None,
            max_route_count: None,
            push_rate_burst: None,
            push_rate_refill_per_sec: None,
            route_idle_ttl_ms: None,
        };
        let env = env_vals(None, None, None, None);
        let cfg = resolve_config(cli, env).unwrap();
        assert_eq!(cfg.bind, "0.0.0.0");
    }

    #[test]
    fn env_bind_can_enable_public_bind() {
        let cli = Cli {
            bind: None,
            port: None,
            max_body_bytes: None,
            max_queue_depth: None,
            max_route_count: None,
            push_rate_burst: None,
            push_rate_refill_per_sec: None,
            route_idle_ttl_ms: None,
        };
        let env = env_vals(Some("0.0.0.0"), None, None, None);
        let cfg = resolve_config(cli, env).unwrap();
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
        let env = env_vals(None, None, Some(0), Some(1));
        let body_err = resolve_config(
            Cli {
                bind: None,
                port: None,
                max_body_bytes: None,
                max_queue_depth: None,
                max_route_count: None,
                push_rate_burst: None,
                push_rate_refill_per_sec: None,
                route_idle_ttl_ms: None,
            },
            env,
        )
        .unwrap_err();
        assert_eq!(body_err, "ERR_INVALID_CONFIG_MAX_BODY_BYTES");

        let env = env_vals(None, None, Some(1), Some(0));
        let depth_err = resolve_config(
            Cli {
                bind: None,
                port: None,
                max_body_bytes: None,
                max_queue_depth: None,
                max_route_count: None,
                push_rate_burst: None,
                push_rate_refill_per_sec: None,
                route_idle_ttl_ms: None,
            },
            env,
        )
        .unwrap_err();
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
