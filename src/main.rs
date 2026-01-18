use std::net::SocketAddr;

use qsl_server::{app, AppState, Limits};
use tokio::net::TcpListener;
use tracing::info;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let port = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(8080);

    let addr: SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
    let listener = TcpListener::bind(addr).await.unwrap();

    let limits = Limits::from_env();
    let state = AppState::new(limits);
    let app = app(state);

    info!("qsl-server listening on {}", addr);
    axum::serve(listener, app).await.unwrap();
}
