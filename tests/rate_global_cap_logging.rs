use qsl_server::{app, AppState, Limits, ResourceControls};
use reqwest::StatusCode as ReqStatus;
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

#[tokio::test(flavor = "current_thread")]
async fn rate_and_route_cap_logs_redact_route_auth_payload() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = set_default(subscriber);

    let auth_token = "NA0280_AUTH_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let wrong_auth = "NA0280_WRONG_AUTH_SENTINEL_MUST_NOT_LEAK";
    let rate_route = "NA0280_RATE_ROUTE_TOKEN_MUST_NOT_LEAK";
    let capped_route = "NA0280_ROUTE_CAP_TOKEN_MUST_NOT_LEAK";
    let accepted_payload = b"NA0280_ACCEPTED_PAYLOAD_MUST_NOT_LEAK".to_vec();
    let rate_payload = b"NA0280_RATE_PAYLOAD_MUST_NOT_LEAK".to_vec();
    let cap_payload = b"NA0280_ROUTE_CAP_PAYLOAD_MUST_NOT_LEAK".to_vec();
    let auth_payload = b"NA0280_AUTH_REJECT_PAYLOAD_MUST_NOT_LEAK".to_vec();

    let limits = Limits::new(128, 4).unwrap();
    let controls = ResourceControls::new(1, 1, 0).unwrap();
    let (base, handle) = spawn_server_with_auth(limits, controls, Some(auth_token)).await;
    let client = reqwest::Client::new();

    let accepted = push(
        &client,
        &base,
        rate_route,
        Some(auth_token),
        Some("NA0280_MSG_ID_NONSECRET_METADATA"),
        accepted_payload,
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let limited = push(
        &client,
        &base,
        rate_route,
        Some(auth_token),
        Some("NA0280_RATE_REJECT_MSG_ID"),
        rate_payload,
    )
    .await;
    assert_eq!(limited.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        limited.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_RATE_LIMITED"
    );

    let capped = push(
        &client,
        &base,
        capped_route,
        Some(auth_token),
        Some("NA0280_ROUTE_CAP_REJECT_MSG_ID"),
        cap_payload,
    )
    .await;
    assert_eq!(capped.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        capped.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_ROUTE_CAP"
    );

    let auth_reject = push(
        &client,
        &base,
        capped_route,
        Some(wrong_auth),
        Some("NA0280_AUTH_REJECT_MSG_ID"),
        auth_payload,
    )
    .await;
    assert_eq!(auth_reject.status(), ReqStatus::UNAUTHORIZED);

    tokio::task::yield_now().await;
    handle.abort();

    let guard = buf.lock().unwrap_or_else(|e| panic!("{e}"));
    let logs = String::from_utf8_lossy(&guard);
    assert!(logs.contains("event=rate_limited"));
    assert!(logs.contains("event=route_cap"));
    assert!(logs.contains("channel_id="));
    assert!(logs.contains("NA0280_MSG_ID_NONSECRET_METADATA"));

    for forbidden in [
        auth_token,
        wrong_auth,
        rate_route,
        capped_route,
        "Authorization",
        "Bearer",
        "NA0280_ACCEPTED_PAYLOAD_MUST_NOT_LEAK",
        "NA0280_RATE_PAYLOAD_MUST_NOT_LEAK",
        "NA0280_ROUTE_CAP_PAYLOAD_MUST_NOT_LEAK",
        "NA0280_AUTH_REJECT_PAYLOAD_MUST_NOT_LEAK",
    ] {
        assert!(!logs.contains(forbidden), "logs leaked {forbidden}");
    }
}
