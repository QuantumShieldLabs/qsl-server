use qsl_server::{app, AppState, Limits, ResourceControls};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use tokio::net::TcpListener;

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const AUTH_HEADER: &str = "Authorization";
const MSG_ID_HEADER: &str = "X-Msg-Id";
const EXACT_4MIB_DATA_CHUNKS: usize = 256;
const DATA_CHUNK_BYTES: usize = 16 * 1024;
const EXACT_4MIB_RELAY_MESSAGES: usize = EXACT_4MIB_DATA_CHUNKS + 1;

#[derive(Deserialize)]
struct PullItem {
    id: String,
    data: Vec<u8>,
}

#[derive(Deserialize)]
struct PullResp {
    items: Vec<PullItem>,
}

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

fn data_chunk_payload() -> Vec<u8> {
    let mut payload = b"NA0598_PAYLOAD_SENTINEL_MUST_NOT_LEAK".to_vec();
    payload.resize(DATA_CHUNK_BYTES, b'd');
    payload
}

#[tokio::test(flavor = "current_thread")]
async fn exact_4mib_legacy_chunks_plus_manifest_fit_default_bounded_queue() {
    let route_token = "NA0598_ROUTE_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let auth_token = "NA0598_AUTH_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let wrong_auth = "NA0598_WRONG_AUTH_SENTINEL_MUST_NOT_LEAK";
    let manifest = b"NA0598_MANIFEST_SENTINEL_MUST_NOT_LEAK".to_vec();
    let (base, handle) = spawn_server(
        Limits::default(),
        ResourceControls::default(),
        Some(auth_token),
    )
    .await;
    let client = reqwest::Client::new();

    let missing_auth = push(
        &client,
        &base,
        route_token,
        None,
        Some("NA0598_MISSING_AUTH_REJECT"),
        b"missing-auth",
    )
    .await;
    assert_eq!(missing_auth.status(), ReqStatus::UNAUTHORIZED);

    let wrong_auth_reject = push(
        &client,
        &base,
        route_token,
        Some(wrong_auth),
        Some("NA0598_WRONG_AUTH_REJECT"),
        b"wrong-auth",
    )
    .await;
    assert_eq!(wrong_auth_reject.status(), ReqStatus::UNAUTHORIZED);

    let payload = data_chunk_payload();
    for idx in 0..EXACT_4MIB_DATA_CHUNKS {
        let msg_id = format!("NA0598_DATA_CHUNK_{idx:03}");
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
        Some("NA0598_MANIFEST_FINAL"),
        manifest.clone(),
    )
    .await;
    assert_eq!(manifest_response.status(), ReqStatus::OK);

    let isolated_pull = pull(
        &client,
        &base,
        "NA0598_ISOLATED_ROUTE",
        Some(auth_token),
        EXACT_4MIB_RELAY_MESSAGES,
    )
    .await;
    assert_eq!(isolated_pull.status(), ReqStatus::NO_CONTENT);

    let delivered = pull(
        &client,
        &base,
        route_token,
        Some(auth_token),
        EXACT_4MIB_RELAY_MESSAGES,
    )
    .await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), EXACT_4MIB_RELAY_MESSAGES);
    assert_eq!(body.items[0].id, "NA0598_DATA_CHUNK_000");
    assert_eq!(body.items[0].data.len(), DATA_CHUNK_BYTES);
    assert_eq!(
        body.items[EXACT_4MIB_DATA_CHUNKS].id,
        "NA0598_MANIFEST_FINAL"
    );
    assert_eq!(body.items[EXACT_4MIB_DATA_CHUNKS].data, manifest);

    let empty_after_drain = pull(&client, &base, route_token, Some(auth_token), 1).await;
    assert_eq!(empty_after_drain.status(), ReqStatus::NO_CONTENT);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn queue_and_push_burst_remain_bounded_after_exact_4mib_shape() {
    let route_token = "NA0598_BOUND_ROUTE";
    let (base, handle) = spawn_server(
        Limits::default(),
        ResourceControls::new(EXACT_4MIB_RELAY_MESSAGES, EXACT_4MIB_RELAY_MESSAGES, 0)
            .unwrap_or_else(|e| panic!("{e}")),
        None,
    )
    .await;
    let client = reqwest::Client::new();

    for idx in 0..EXACT_4MIB_RELAY_MESSAGES {
        let response = push(
            &client,
            &base,
            route_token,
            None,
            Some(&format!("NA0598_BOUND_{idx:03}")),
            b"bounded".to_vec(),
        )
        .await;
        assert_eq!(response.status(), ReqStatus::OK, "bounded push {idx}");
    }

    let beyond = push(
        &client,
        &base,
        route_token,
        None,
        Some("NA0598_BEYOND_BOUND"),
        b"beyond".to_vec(),
    )
    .await;
    assert_eq!(beyond.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        beyond.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_OVERLOADED"
    );

    let drained = pull(&client, &base, route_token, None, EXACT_4MIB_RELAY_MESSAGES).await;
    assert_eq!(drained.status(), ReqStatus::OK);
    let body: PullResp = drained.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), EXACT_4MIB_RELAY_MESSAGES);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn attachment_descriptor_payload_remains_opaque_single_relay_message() {
    let route_token = "NA0598_ATTACHMENT_DESCRIPTOR_ROUTE";
    let descriptor = br#"{"kind":"qsl-attachments-descriptor","ciphertext_len":4194305}"#.to_vec();
    let (base, handle) = spawn_server(Limits::default(), ResourceControls::default(), None).await;
    let client = reqwest::Client::new();

    let pushed = push(
        &client,
        &base,
        route_token,
        None,
        Some("NA0598_ATTACHMENT_DESCRIPTOR"),
        descriptor.clone(),
    )
    .await;
    assert_eq!(pushed.status(), ReqStatus::OK);

    let pulled = pull(&client, &base, route_token, None, 1).await;
    assert_eq!(pulled.status(), ReqStatus::OK);
    let body: PullResp = pulled.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0598_ATTACHMENT_DESCRIPTOR");
    assert_eq!(body.items[0].data, descriptor);

    handle.abort();
}
