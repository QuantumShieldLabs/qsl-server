use qsl_server::{app, AppState, Limits, ResourceControls};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use std::time::Duration;
use tokio::net::TcpListener;

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const MSG_ID_HEADER: &str = "X-Msg-Id";
const TTL_MS: usize = 25;
const TTL_MARGIN: Duration = Duration::from_millis(75);

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

fn ttl_controls(max_route_count: usize, push_rate_burst: usize) -> ResourceControls {
    ResourceControls::new_with_route_idle_ttl_ms(max_route_count, push_rate_burst, 0, TTL_MS)
        .unwrap()
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
async fn unknown_pull_does_not_create_route_slot() {
    let (base, handle) =
        spawn_server_with_auth(Limits::new(128, 4).unwrap(), ttl_controls(1, 4), None).await;
    let client = reqwest::Client::new();

    let unknown = pull(&client, &base, "NA0281_UNKNOWN_PULL", None, 1).await;
    assert_eq!(unknown.status(), ReqStatus::NO_CONTENT);

    let accepted = push(
        &client,
        &base,
        "NA0281_AFTER_UNKNOWN_PULL",
        None,
        Some("NA0281_AFTER_UNKNOWN_PULL_ID"),
        b"accepted-after-unknown".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let delivered = pull(&client, &base, "NA0281_AFTER_UNKNOWN_PULL", None, 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0281_AFTER_UNKNOWN_PULL_ID");
    assert_eq!(body.items[0].data.as_slice(), b"accepted-after-unknown");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn drain_to_empty_releases_route_slot_and_rate_bucket() {
    let (base, handle) =
        spawn_server_with_auth(Limits::new(128, 4).unwrap(), ttl_controls(1, 1), None).await;
    let client = reqwest::Client::new();

    let first = push(
        &client,
        &base,
        "NA0281_DRAIN_RATE_ROUTE",
        None,
        Some("NA0281_DRAIN_RATE_FIRST"),
        b"first".to_vec(),
    )
    .await;
    assert_eq!(first.status(), ReqStatus::OK);

    let limited = push(
        &client,
        &base,
        "NA0281_DRAIN_RATE_ROUTE",
        None,
        Some("NA0281_DRAIN_RATE_LIMITED"),
        b"limited".to_vec(),
    )
    .await;
    assert_eq!(limited.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        limited.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_RATE_LIMITED"
    );

    let drained = pull(&client, &base, "NA0281_DRAIN_RATE_ROUTE", None, 1).await;
    assert_eq!(drained.status(), ReqStatus::OK);

    let reused = push(
        &client,
        &base,
        "NA0281_DRAIN_RATE_ROUTE",
        None,
        Some("NA0281_DRAIN_RATE_REUSED"),
        b"reused".to_vec(),
    )
    .await;
    assert_eq!(reused.status(), ReqStatus::OK);

    let delivered = pull(&client, &base, "NA0281_DRAIN_RATE_ROUTE", None, 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0281_DRAIN_RATE_REUSED");
    assert_eq!(body.items[0].data.as_slice(), b"reused");

    let other_route = push(
        &client,
        &base,
        "NA0281_DRAIN_RELEASED_SLOT",
        None,
        Some("NA0281_DRAIN_SLOT_ID"),
        b"slot".to_vec(),
    )
    .await;
    assert_eq!(other_route.status(), ReqStatus::OK);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn idle_route_ttl_releases_capacity() {
    let (base, handle) =
        spawn_server_with_auth(Limits::new(128, 4).unwrap(), ttl_controls(1, 2), None).await;
    let client = reqwest::Client::new();

    let stale = push(
        &client,
        &base,
        "NA0281_TTL_STALE_ROUTE",
        None,
        Some("NA0281_TTL_STALE_ID"),
        b"stale".to_vec(),
    )
    .await;
    assert_eq!(stale.status(), ReqStatus::OK);

    tokio::time::sleep(TTL_MARGIN).await;

    let fresh = push(
        &client,
        &base,
        "NA0281_TTL_FRESH_ROUTE",
        None,
        Some("NA0281_TTL_FRESH_ID"),
        b"fresh".to_vec(),
    )
    .await;
    assert_eq!(fresh.status(), ReqStatus::OK);

    let expired = pull(&client, &base, "NA0281_TTL_STALE_ROUTE", None, 1).await;
    assert_eq!(expired.status(), ReqStatus::NO_CONTENT);

    let delivered = pull(&client, &base, "NA0281_TTL_FRESH_ROUTE", None, 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0281_TTL_FRESH_ID");
    assert_eq!(body.items[0].data.as_slice(), b"fresh");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn expired_route_does_not_return_stale_message() {
    let (base, handle) =
        spawn_server_with_auth(Limits::new(128, 4).unwrap(), ttl_controls(2, 2), None).await;
    let client = reqwest::Client::new();

    let stale = push(
        &client,
        &base,
        "NA0281_TTL_NO_STALE_ROUTE",
        None,
        Some("NA0281_TTL_NO_STALE_ID"),
        b"stale-message".to_vec(),
    )
    .await;
    assert_eq!(stale.status(), ReqStatus::OK);

    tokio::time::sleep(TTL_MARGIN).await;

    let expired = pull(&client, &base, "NA0281_TTL_NO_STALE_ROUTE", None, 1).await;
    assert_eq!(expired.status(), ReqStatus::NO_CONTENT);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn expired_route_releases_rate_bucket() {
    let (base, handle) =
        spawn_server_with_auth(Limits::new(128, 4).unwrap(), ttl_controls(2, 1), None).await;
    let client = reqwest::Client::new();

    let accepted = push(
        &client,
        &base,
        "NA0281_TTL_RATE_ROUTE",
        None,
        Some("NA0281_TTL_RATE_STALE_ID"),
        b"stale".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let limited = push(
        &client,
        &base,
        "NA0281_TTL_RATE_ROUTE",
        None,
        Some("NA0281_TTL_RATE_LIMITED_ID"),
        b"limited".to_vec(),
    )
    .await;
    assert_eq!(limited.status(), ReqStatus::TOO_MANY_REQUESTS);

    tokio::time::sleep(TTL_MARGIN).await;

    let fresh = push(
        &client,
        &base,
        "NA0281_TTL_RATE_ROUTE",
        None,
        Some("NA0281_TTL_RATE_FRESH_ID"),
        b"fresh".to_vec(),
    )
    .await;
    assert_eq!(fresh.status(), ReqStatus::OK);

    let delivered = pull(&client, &base, "NA0281_TTL_RATE_ROUTE", None, 2).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0281_TTL_RATE_FRESH_ID");
    assert_eq!(body.items[0].data.as_slice(), b"fresh");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn push_after_expiry_does_not_resurrect_old_messages() {
    let (base, handle) =
        spawn_server_with_auth(Limits::new(128, 4).unwrap(), ttl_controls(2, 2), None).await;
    let client = reqwest::Client::new();

    let stale = push(
        &client,
        &base,
        "NA0281_TTL_REUSE_ROUTE",
        None,
        Some("NA0281_TTL_REUSE_STALE_ID"),
        b"stale".to_vec(),
    )
    .await;
    assert_eq!(stale.status(), ReqStatus::OK);

    tokio::time::sleep(TTL_MARGIN).await;

    let fresh = push(
        &client,
        &base,
        "NA0281_TTL_REUSE_ROUTE",
        None,
        Some("NA0281_TTL_REUSE_FRESH_ID"),
        b"fresh".to_vec(),
    )
    .await;
    assert_eq!(fresh.status(), ReqStatus::OK);

    let delivered = pull(&client, &base, "NA0281_TTL_REUSE_ROUTE", None, 2).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0281_TTL_REUSE_FRESH_ID");
    assert_eq!(body.items[0].data.as_slice(), b"fresh");

    handle.abort();
}

#[test]
fn route_ttl_config_and_docs_are_explicit() {
    let controls = ResourceControls::new_with_route_idle_ttl_ms(4, 4, 0, TTL_MS).unwrap();
    assert_eq!(controls.route_idle_ttl.as_millis(), TTL_MS as u128);
    assert!(ResourceControls::new_with_route_idle_ttl_ms(4, 4, 0, 0).is_err());

    let readme = include_str!("../README.md");
    let inbox_contract =
        include_str!("../docs/server/DOC-SRV-003_Relay_Inbox_Contract_v1.0.0_DRAFT.md");

    for doc in [readme, inbox_contract] {
        assert!(doc.contains("ROUTE_IDLE_TTL_MS"));
        assert!(doc.contains("ERR_ROUTE_CAP"));
        assert!(doc.contains("Time-based idle TTL"));
    }
}
