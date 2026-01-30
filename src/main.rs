use std::{env, net::SocketAddr};

use clap::Parser;
use qsl_server::{app, AppState, Limits, MAX_BODY_BYTES_CEILING, MAX_QUEUE_DEPTH_CEILING};
use tokio::net::TcpListener;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "qsl-server", version)]
struct Cli {
    /// Bind address (default: 0.0.0.0)
    #[arg(long)]
    bind: Option<String>,
    /// Listen port (env: PORT, default: 8080)
    #[arg(long)]
    port: Option<u16>,
    /// Max body bytes (env: MAX_BODY_BYTES, default: 1048576)
    #[arg(long)]
    max_body_bytes: Option<usize>,
    /// Max queue depth (env: MAX_QUEUE_DEPTH, default: 256)
    #[arg(long)]
    max_queue_depth: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct EnvVals {
    port: Option<u16>,
    max_body_bytes: Option<usize>,
    max_queue_depth: Option<usize>,
}

impl EnvVals {
    fn from_env() -> Self {
        Self {
            port: env_u16("PORT"),
            max_body_bytes: env_usize("MAX_BODY_BYTES"),
            max_queue_depth: env_usize("MAX_QUEUE_DEPTH"),
        }
    }
}

#[derive(Clone, Debug)]
struct Config {
    bind: String,
    port: u16,
    limits: Limits,
}

fn env_u16(name: &str) -> Option<u16> {
    env::var(name).ok().and_then(|v| v.parse::<u16>().ok())
}

fn env_usize(name: &str) -> Option<usize> {
    env::var(name).ok().and_then(|v| v.parse::<usize>().ok())
}

fn resolve_config(cli: Cli, env: EnvVals) -> Config {
    let bind = cli.bind.unwrap_or_else(|| "0.0.0.0".to_string());
    let port = cli.port.or(env.port).unwrap_or(8080);
    let max_body_bytes = cli
        .max_body_bytes
        .or(env.max_body_bytes)
        .unwrap_or(MAX_BODY_BYTES_CEILING)
        .min(MAX_BODY_BYTES_CEILING);
    let max_queue_depth = cli
        .max_queue_depth
        .or(env.max_queue_depth)
        .unwrap_or(MAX_QUEUE_DEPTH_CEILING)
        .min(MAX_QUEUE_DEPTH_CEILING);
    Config {
        bind,
        port,
        limits: Limits {
            max_body_bytes,
            max_queue_depth,
        },
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let cfg = resolve_config(cli, EnvVals::from_env());
    let addr: SocketAddr = format!("{}:{}", cfg.bind, cfg.port).parse().unwrap();
    let listener = TcpListener::bind(addr).await.unwrap();

    let state = AppState::new(cfg.limits);
    let app = app(state);

    info!("qsl-server listening on {}", addr);
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn env_vals(
        port: Option<u16>,
        max_body_bytes: Option<usize>,
        max_queue_depth: Option<usize>,
    ) -> EnvVals {
        EnvVals {
            port,
            max_body_bytes,
            max_queue_depth,
        }
    }

    #[test]
    fn cli_overrides_env() {
        let cli = Cli {
            bind: None,
            port: Some(9000),
            max_body_bytes: Some(4096),
            max_queue_depth: Some(9),
        };
        let env = env_vals(Some(8080), Some(1024), Some(1));
        let cfg = resolve_config(cli, env);
        assert_eq!(cfg.port, 9000);
        assert_eq!(cfg.limits.max_body_bytes, 4096);
        assert_eq!(cfg.limits.max_queue_depth, 9);
    }

    #[test]
    fn env_overrides_defaults() {
        let cli = Cli {
            bind: None,
            port: None,
            max_body_bytes: None,
            max_queue_depth: None,
        };
        let env = env_vals(Some(7070), Some(2048), Some(7));
        let cfg = resolve_config(cli, env);
        assert_eq!(cfg.port, 7070);
        assert_eq!(cfg.limits.max_body_bytes, 2048);
        assert_eq!(cfg.limits.max_queue_depth, 7);
    }

    #[test]
    fn limits_are_capped() {
        let cli = Cli {
            bind: None,
            port: None,
            max_body_bytes: Some(MAX_BODY_BYTES_CEILING * 2),
            max_queue_depth: Some(MAX_QUEUE_DEPTH_CEILING * 2),
        };
        let env = env_vals(None, None, None);
        let cfg = resolve_config(cli, env);
        assert_eq!(cfg.limits.max_body_bytes, MAX_BODY_BYTES_CEILING);
        assert_eq!(cfg.limits.max_queue_depth, MAX_QUEUE_DEPTH_CEILING);
    }
}
