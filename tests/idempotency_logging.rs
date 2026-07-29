use qsl_server::{app, AppState, Limits};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use tokio::net::TcpListener;
use tracing::subscriber::set_default;

mod common;
use common::{await_logs, capture};

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const MSG_ID_HEADER: &str = "X-Msg-Id";

#[derive(Deserialize)]
struct PullItem {
    id: String,
    data: Vec<u8>,
}

#[derive(Deserialize)]
struct PullResp {
    items: Vec<PullItem>,
}

async fn spawn_server_with_auth(
    limits: Limits,
    relay_token: Option<&str>,
) -> (String, tokio::task::JoinHandle<()>) {
    let state = AppState::new_with_auth(limits, relay_token.map(str::to_string));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let addr = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
    let handle = tokio::spawn(async move {
        axum::serve(listener, app(state))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    });
    (format!("http://{addr}"), handle)
}

#[tokio::test(flavor = "current_thread")]
async fn x_msg_id_log_boundary_is_metadata_only() {
    let (buf, writer) = capture();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = set_default(subscriber);

    let route_token = "NA0275_ROUTE_TOKEN_SENTINEL";
    let auth_token = "NA0275_AUTH_TOKEN_SENTINEL";
    let payload = b"NA0275_PAYLOAD_SENTINEL".to_vec();
    let msg_id = "NA0275_MSG_ID_NONSECRET_METADATA";

    let (base, handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 1024,
            max_queue_depth: 8,
        },
        Some(auth_token),
    )
    .await;
    let client = reqwest::Client::new();

    let accepted = client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .header("Authorization", format!("Bearer {auth_token}"))
        .header(MSG_ID_HEADER, msg_id)
        .body(payload)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(accepted.status(), ReqStatus::OK);

    let delivered = client
        .get(format!("{base}/v1/pull?max=1"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .header("Authorization", format!("Bearer {auth_token}"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, msg_id);
    assert_eq!(body.items[0].data.as_slice(), b"NA0275_PAYLOAD_SENTINEL");

    // NA-0687: await the relay's own log lines BEFORE aborting the task. abort()
    // guarantees a not-yet-emitted line is never emitted, so a wait placed after it
    // could not succeed. The single `yield_now()` this replaces gave the server task
    // exactly one scheduling opportunity -- a nudge, not a synchronisation.
    let logs = await_logs(&buf, &["push channel_id=", msg_id]).await;
    handle.abort();

    assert!(logs.contains("push channel_id="));
    assert!(logs.contains(msg_id));

    for forbidden in [
        route_token,
        auth_token,
        "Authorization",
        "Bearer",
        "NA0275_PAYLOAD_SENTINEL",
    ] {
        assert!(!logs.contains(forbidden), "logs leaked {forbidden}");
    }
}
