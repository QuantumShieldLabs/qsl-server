use qsl_server::{app, AppState, Limits, ResourceControls};
use reqwest::StatusCode as ReqStatus;
use tokio::net::TcpListener;
use tracing::subscriber::set_default;

mod common;
use common::{await_logs, capture};

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const AUTH_HEADER: &str = "Authorization";
const MSG_ID_HEADER: &str = "X-Msg-Id";
const EXACT_4MIB_DATA_CHUNKS: usize = 256;
const DATA_CHUNK_BYTES: usize = 16 * 1024;

async fn spawn_server(
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
    assert!(addr.ip().is_loopback());
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
        request = request.header(AUTH_HEADER, format!("Bearer {token}"));
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
        request = request.header(AUTH_HEADER, format!("Bearer {token}"));
    }
    request.send().await.unwrap_or_else(|e| panic!("{e}"))
}

#[tokio::test(flavor = "current_thread")]
async fn exact_4mib_relay_logs_remain_metadata_only() {
    let (buf, writer) = capture();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = set_default(subscriber);

    let route_token = "NA0598_LOG_ROUTE_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let auth_token = "NA0598_LOG_AUTH_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let wrong_auth = "NA0598_LOG_WRONG_AUTH_SENTINEL_MUST_NOT_LEAK";
    let mut payload = b"NA0598_LOG_PAYLOAD_SENTINEL_MUST_NOT_LEAK".to_vec();
    payload.resize(DATA_CHUNK_BYTES, b'd');
    let manifest = b"NA0598_LOG_MANIFEST_SENTINEL_MUST_NOT_LEAK".to_vec();

    let (base, handle) = spawn_server(
        Limits::default(),
        ResourceControls::default(),
        Some(auth_token),
    )
    .await;
    let client = reqwest::Client::new();

    let auth_reject = push(
        &client,
        &base,
        route_token,
        Some(wrong_auth),
        Some("NA0598_LOG_WRONG_AUTH_REJECT"),
        b"NA0598_LOG_REJECT_PAYLOAD_SENTINEL_MUST_NOT_LEAK",
    )
    .await;
    assert_eq!(auth_reject.status(), ReqStatus::UNAUTHORIZED);

    for idx in 0..EXACT_4MIB_DATA_CHUNKS {
        let msg_id = format!("NA0598_LOG_DATA_{idx:03}");
        let response = push(
            &client,
            &base,
            route_token,
            Some(auth_token),
            Some(&msg_id),
            payload.clone(),
        )
        .await;
        assert_eq!(response.status(), ReqStatus::OK, "data chunk {idx}");
    }

    let manifest_response = push(
        &client,
        &base,
        route_token,
        Some(auth_token),
        Some("NA0598_LOG_MANIFEST_FINAL"),
        manifest,
    )
    .await;
    assert_eq!(manifest_response.status(), ReqStatus::OK);

    let delivered = pull(
        &client,
        &base,
        route_token,
        Some(auth_token),
        EXACT_4MIB_DATA_CHUNKS + 1,
    )
    .await;
    assert_eq!(delivered.status(), ReqStatus::OK);

    // NA-0687: await the relay's own log lines BEFORE aborting the task. abort()
    // guarantees a not-yet-emitted line is never emitted, so a wait placed after it
    // could not succeed. The single `yield_now()` this replaces gave the server task
    // exactly one scheduling opportunity -- a nudge, not a synchronisation.
    let logs = await_logs(
        &buf,
        &[
            "push channel_id=",
            "pull channel_id=",
            "NA0598_LOG_MANIFEST_FINAL",
        ],
    )
    .await;
    handle.abort();

    assert!(logs.contains("push channel_id="));
    assert!(logs.contains("pull channel_id="));
    assert!(logs.contains("NA0598_LOG_MANIFEST_FINAL"));
    for forbidden in [
        route_token,
        auth_token,
        wrong_auth,
        "Authorization",
        "Bearer",
        "NA0598_LOG_PAYLOAD_SENTINEL_MUST_NOT_LEAK",
        "NA0598_LOG_REJECT_PAYLOAD_SENTINEL_MUST_NOT_LEAK",
        "NA0598_LOG_MANIFEST_SENTINEL_MUST_NOT_LEAK",
    ] {
        assert!(!logs.contains(forbidden), "logs leaked {forbidden}");
    }
}
