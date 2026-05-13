use qsl_server::{app, AppState, Limits, ResourceControls};
use reqwest::StatusCode as ReqStatus;
use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
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

async fn spawn_server_with_auth(
    limits: Limits,
    controls: ResourceControls,
    relay_token: Option<&str>,
) -> (String, tokio::task::JoinHandle<()>) {
    let state =
        AppState::new_with_auth_and_controls(limits, controls, relay_token.map(str::to_string));
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

async fn push(
    client: &reqwest::Client,
    base: &str,
    route_token: &str,
    auth_token: Option<&str>,
    msg_id: Option<&str>,
    body: impl Into<Vec<u8>>,
) -> reqwest::Response {
    let mut request = client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .body(body.into());
    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    if let Some(id) = msg_id {
        request = request.header(MSG_ID_HEADER, id);
    }
    request.send().await.unwrap_or_else(|e| panic!("{e}"))
}

async fn pull(
    client: &reqwest::Client,
    base: &str,
    route_token: &str,
    auth_token: Option<&str>,
    max: usize,
) -> reqwest::Response {
    let mut request = client
        .get(format!("{base}/v1/pull?max={max}"))
        .header(ROUTE_TOKEN_HEADER, route_token);
    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    request.send().await.unwrap_or_else(|e| panic!("{e}"))
}

#[tokio::test(flavor = "current_thread")]
async fn ttl_cleanup_logs_redact_route_auth_payload() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = set_default(subscriber);

    let auth_token = "NA0281_AUTH_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let route_token = "NA0281_TTL_ROUTE_TOKEN_MUST_NOT_LEAK";
    let payload = b"NA0281_TTL_PAYLOAD_MUST_NOT_LEAK".to_vec();
    let controls = ResourceControls::new_with_route_idle_ttl_ms(2, 2, 0, 25).unwrap();
    let (base, handle) =
        spawn_server_with_auth(Limits::new(128, 4).unwrap(), controls, Some(auth_token)).await;
    let client = reqwest::Client::new();

    let accepted = push(
        &client,
        &base,
        route_token,
        Some(auth_token),
        Some("NA0281_TTL_MSG_ID_NONSECRET_METADATA"),
        payload,
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    tokio::time::sleep(Duration::from_millis(75)).await;

    let expired = pull(&client, &base, route_token, Some(auth_token), 1).await;
    assert_eq!(expired.status(), ReqStatus::NO_CONTENT);

    tokio::task::yield_now().await;
    handle.abort();

    let guard = buf.lock().unwrap_or_else(|e| panic!("{e}"));
    let logs = String::from_utf8_lossy(&guard);
    assert!(logs.contains("event=route_expired"));
    assert!(logs.contains("channel_id="));
    assert!(logs.contains("queued_messages=1"));
    assert!(logs.contains("ttl_ms=25"));
    assert!(logs.contains("NA0281_TTL_MSG_ID_NONSECRET_METADATA"));

    for forbidden in [
        auth_token,
        route_token,
        "Authorization",
        "Bearer",
        "NA0281_TTL_PAYLOAD_MUST_NOT_LEAK",
    ] {
        assert!(!logs.contains(forbidden), "logs leaked {forbidden}");
    }
}
