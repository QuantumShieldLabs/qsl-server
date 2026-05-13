use qsl_server::{app, AppState, Limits};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;
use tracing::subscriber::set_default;

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const MSG_ID_HEADER: &str = "X-Msg-Id";

#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut guard = self.0.lock().unwrap_or_else(|e| panic!("{e}"));
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

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
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());
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

    tokio::task::yield_now().await;
    handle.abort();

    let guard = buf.lock().unwrap_or_else(|e| panic!("{e}"));
    let logs = String::from_utf8_lossy(&guard);
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
