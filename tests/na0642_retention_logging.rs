// NA-0642: replaces the retired route_lifecycle_ttl_logging redaction test.
// Retention-expiry logs must stay metadata-only: redacted channel id, counts,
// TTL — never the route token, auth token, or payload.

use qsl_server::{app, AppState, Limits, ResourceControls, StoreConfig};
use reqwest::StatusCode as ReqStatus;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing::subscriber::set_default;

mod common;
use common::{await_logs, capture, install_permissive_global_once};

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const MSG_ID_HEADER: &str = "X-Msg-Id";

#[tokio::test(flavor = "current_thread")]
async fn retention_cleanup_logs_redact_route_auth_payload() {
    install_permissive_global_once();
    let (buf, writer) = capture();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = set_default(subscriber);

    let auth_token = "NA0642_AUTH_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let route_token = "NA0642_RETENTION_ROUTE_TOKEN_MUST_NOT_LEAK";
    let payload = b"NA0642_RETENTION_PAYLOAD_MUST_NOT_LEAK".to_vec();
    let controls = ResourceControls::new_with_route_idle_ttl_ms(2, 2, 0, 25).unwrap();
    let store = StoreConfig {
        retention_ttl_secs: 1,
        ..StoreConfig::default()
    };
    let state = AppState::new_with_auth_controls_and_store(
        Limits::new(128, 4).unwrap(),
        controls,
        Some(auth_token.to_string()),
        store,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let addr = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
    let handle = tokio::spawn(async move {
        axum::serve(listener, app(state))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    });
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let accepted = client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .header(MSG_ID_HEADER, "NA0642_RETENTION_MSG_ID_NONSECRET_METADATA")
        .header("Authorization", format!("Bearer {auth_token}"))
        .body(payload)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(accepted.status(), ReqStatus::OK);

    tokio::time::sleep(Duration::from_millis(1500)).await;

    let expired = client
        .get(format!("{base}/v1/pull?max=1"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .header("Authorization", format!("Bearer {auth_token}"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(expired.status(), ReqStatus::NO_CONTENT);

    // NA-0687: await the relay's own log lines BEFORE aborting the task. abort()
    // guarantees a not-yet-emitted line is never emitted, so a wait placed after it
    // could not succeed. The single `yield_now()` this replaces gave the server task
    // exactly one scheduling opportunity -- a nudge, not a synchronisation.
    let logs = await_logs(
        &buf,
        &[
            "event=retention_expired",
            "channel_id=",
            "expired_messages=1",
            "ttl_secs=1",
        ],
    )
    .await;
    handle.abort();

    assert!(logs.contains("event=retention_expired"));
    assert!(logs.contains("channel_id="));
    assert!(logs.contains("expired_messages=1"));
    assert!(logs.contains("ttl_secs=1"));

    for forbidden in [
        auth_token,
        route_token,
        "Authorization",
        "Bearer",
        "NA0642_RETENTION_PAYLOAD_MUST_NOT_LEAK",
    ] {
        assert!(!logs.contains(forbidden), "logs leaked {forbidden}");
    }
}
