// NA-0642: message lifetime is governed by the store's retention TTL; the
// 5-minute idle-route discard (NA-0281 route_lifecycle_ttl) is retired. The
// drain-release contracts from NA-0281 are carried forward here unchanged.

use qsl_server::{app, AppState, Limits, ResourceControls, StoreConfig};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use std::time::Duration;
use tokio::net::TcpListener;

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

fn short_retention() -> StoreConfig {
    StoreConfig {
        retention_ttl_secs: 1,
        ..StoreConfig::default()
    }
}

async fn spawn_server(
    limits: Limits,
    controls: ResourceControls,
    store: StoreConfig,
) -> (String, AppState, tokio::task::JoinHandle<()>) {
    let state = AppState::new_with_auth_controls_and_store(limits, controls, None, store)
        .unwrap_or_else(|e| panic!("{e}"));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let addr = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
    let served = state.clone();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app(served))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    });
    (format!("http://{addr}"), state, handle)
}

fn rate_controls(max_route_count: usize, push_rate_burst: usize) -> ResourceControls {
    ResourceControls::new_with_route_idle_ttl_ms(max_route_count, push_rate_burst, 0, 25)
        .unwrap_or_else(|e| panic!("{e}"))
}

async fn push(
    client: &reqwest::Client,
    base: &str,
    route_token: &str,
    msg_id: Option<&str>,
    body: impl Into<Vec<u8>>,
) -> reqwest::Response {
    let mut request = client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .body(body.into());
    if let Some(id) = msg_id {
        request = request.header(MSG_ID_HEADER, id);
    }
    request.send().await.unwrap_or_else(|e| panic!("{e}"))
}

async fn pull(
    client: &reqwest::Client,
    base: &str,
    route_token: &str,
    max: usize,
) -> reqwest::Response {
    client
        .get(format!("{base}/v1/pull?max={max}"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
}

#[tokio::test(flavor = "current_thread")]
async fn pull_unknown_route_returns_204_then_push_works() {
    let (base, _state, handle) = spawn_server(
        Limits::new(128, 4).unwrap(),
        rate_controls(4, 4),
        StoreConfig::default(),
    )
    .await;
    let client = reqwest::Client::new();

    let unknown = pull(&client, &base, "NA0281_AFTER_UNKNOWN_PULL", 1).await;
    assert_eq!(unknown.status(), ReqStatus::NO_CONTENT);

    let accepted = push(
        &client,
        &base,
        "NA0281_AFTER_UNKNOWN_PULL",
        Some("NA0281_AFTER_UNKNOWN_PULL_ID"),
        b"accepted-after-unknown".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let delivered = pull(&client, &base, "NA0281_AFTER_UNKNOWN_PULL", 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0281_AFTER_UNKNOWN_PULL_ID");
    assert_eq!(body.items[0].data.as_slice(), b"accepted-after-unknown");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn drain_to_empty_releases_route_slot_and_rate_bucket() {
    let (base, _state, handle) = spawn_server(
        Limits::new(128, 4).unwrap(),
        rate_controls(1, 1),
        StoreConfig::default(),
    )
    .await;
    let client = reqwest::Client::new();

    let first = push(
        &client,
        &base,
        "NA0281_DRAIN_RATE_ROUTE",
        Some("NA0281_DRAIN_RATE_FIRST"),
        b"first".to_vec(),
    )
    .await;
    assert_eq!(first.status(), ReqStatus::OK);

    let limited = push(
        &client,
        &base,
        "NA0281_DRAIN_RATE_ROUTE",
        Some("NA0281_DRAIN_RATE_LIMITED"),
        b"limited".to_vec(),
    )
    .await;
    assert_eq!(limited.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        limited.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_RATE_LIMITED"
    );

    let drained = pull(&client, &base, "NA0281_DRAIN_RATE_ROUTE", 1).await;
    assert_eq!(drained.status(), ReqStatus::OK);

    let reused = push(
        &client,
        &base,
        "NA0281_DRAIN_RATE_ROUTE",
        Some("NA0281_DRAIN_RATE_REUSED"),
        b"reused".to_vec(),
    )
    .await;
    assert_eq!(reused.status(), ReqStatus::OK);

    let delivered = pull(&client, &base, "NA0281_DRAIN_RATE_ROUTE", 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0281_DRAIN_RATE_REUSED");
    assert_eq!(body.items[0].data.as_slice(), b"reused");

    let other_route = push(
        &client,
        &base,
        "NA0281_DRAIN_RELEASED_SLOT",
        Some("NA0281_DRAIN_SLOT_ID"),
        b"slot".to_vec(),
    )
    .await;
    assert_eq!(other_route.status(), ReqStatus::OK);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn retention_ttl_expires_undelivered_messages() {
    let (base, _state, handle) = spawn_server(
        Limits::new(128, 4).unwrap(),
        rate_controls(4, 4),
        short_retention(),
    )
    .await;
    let client = reqwest::Client::new();

    let stale = push(
        &client,
        &base,
        "NA0642_RETENTION_ROUTE",
        Some("NA0642_RETENTION_STALE_ID"),
        b"stale".to_vec(),
    )
    .await;
    assert_eq!(stale.status(), ReqStatus::OK);

    tokio::time::sleep(Duration::from_millis(1500)).await;

    let expired = pull(&client, &base, "NA0642_RETENTION_ROUTE", 1).await;
    assert_eq!(expired.status(), ReqStatus::NO_CONTENT);

    // Non-vacuity: the pull path still delivers on the same server after the
    // expiry window — the stale message vanished because of retention, not
    // because delivery broke.
    let fresh = push(
        &client,
        &base,
        "NA0642_RETENTION_ROUTE",
        Some("NA0642_RETENTION_FRESH_ID"),
        b"fresh".to_vec(),
    )
    .await;
    assert_eq!(fresh.status(), ReqStatus::OK);
    let delivered = pull(&client, &base, "NA0642_RETENTION_ROUTE", 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0642_RETENTION_FRESH_ID");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn default_retention_does_not_expire_quickly() {
    // Negative control for the expiry test: with the default TTL a message
    // must survive the same sleep the short-TTL test uses.
    let (base, _state, handle) = spawn_server(
        Limits::new(128, 4).unwrap(),
        rate_controls(4, 4),
        StoreConfig::default(),
    )
    .await;
    let client = reqwest::Client::new();

    let accepted = push(
        &client,
        &base,
        "NA0642_RETENTION_CONTROL_ROUTE",
        Some("NA0642_RETENTION_CONTROL_ID"),
        b"survives".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    tokio::time::sleep(Duration::from_millis(1500)).await;

    let delivered = pull(&client, &base, "NA0642_RETENTION_CONTROL_ROUTE", 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0642_RETENTION_CONTROL_ID");
    assert_eq!(body.items[0].data.as_slice(), b"survives");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn retention_sweep_entry_point_expires_and_reports() {
    // The periodic background sweep uses this entry point; quiet relays must
    // expire without any push/pull traffic triggering the lazy path.
    let (base, state, handle) = spawn_server(
        Limits::new(128, 8).unwrap(),
        rate_controls(4, 8),
        short_retention(),
    )
    .await;
    let client = reqwest::Client::new();

    for id in ["NA0642_SWEEP_A", "NA0642_SWEEP_B"] {
        let accepted = push(
            &client,
            &base,
            "NA0642_SWEEP_ROUTE",
            Some(id),
            b"sweepable".to_vec(),
        )
        .await;
        assert_eq!(accepted.status(), ReqStatus::OK);
    }

    tokio::time::sleep(Duration::from_millis(1500)).await;

    let stats = state.run_retention_sweep();
    assert_eq!(stats.expired_messages, 2);
    assert_eq!(stats.expired_routes.len(), 1);
    assert_eq!(stats.removed_route_keys.len(), 1);

    let after = pull(&client, &base, "NA0642_SWEEP_ROUTE", 2).await;
    assert_eq!(after.status(), ReqStatus::NO_CONTENT);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn push_after_expiry_does_not_resurrect_old_messages() {
    let (base, _state, handle) = spawn_server(
        Limits::new(128, 4).unwrap(),
        rate_controls(4, 4),
        short_retention(),
    )
    .await;
    let client = reqwest::Client::new();

    let stale = push(
        &client,
        &base,
        "NA0642_REUSE_ROUTE",
        Some("NA0642_REUSE_STALE_ID"),
        b"stale".to_vec(),
    )
    .await;
    assert_eq!(stale.status(), ReqStatus::OK);

    tokio::time::sleep(Duration::from_millis(1500)).await;

    let fresh = push(
        &client,
        &base,
        "NA0642_REUSE_ROUTE",
        Some("NA0642_REUSE_FRESH_ID"),
        b"fresh".to_vec(),
    )
    .await;
    assert_eq!(fresh.status(), ReqStatus::OK);

    let delivered = pull(&client, &base, "NA0642_REUSE_ROUTE", 2).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0642_REUSE_FRESH_ID");
    assert_eq!(body.items[0].data.as_slice(), b"fresh");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn retention_expiry_releases_route_slot() {
    // Replaces the retired idle-TTL capacity release: an expired route frees
    // its MAX_ROUTE_COUNT slot via the lazy sweep on the next push.
    let (base, _state, handle) = spawn_server(
        Limits::new(128, 4).unwrap(),
        rate_controls(1, 2),
        short_retention(),
    )
    .await;
    let client = reqwest::Client::new();

    let stale = push(
        &client,
        &base,
        "NA0642_SLOT_STALE_ROUTE",
        Some("NA0642_SLOT_STALE_ID"),
        b"stale".to_vec(),
    )
    .await;
    assert_eq!(stale.status(), ReqStatus::OK);

    tokio::time::sleep(Duration::from_millis(1500)).await;

    let fresh = push(
        &client,
        &base,
        "NA0642_SLOT_FRESH_ROUTE",
        Some("NA0642_SLOT_FRESH_ID"),
        b"fresh".to_vec(),
    )
    .await;
    assert_eq!(fresh.status(), ReqStatus::OK);

    let expired = pull(&client, &base, "NA0642_SLOT_STALE_ROUTE", 1).await;
    assert_eq!(expired.status(), ReqStatus::NO_CONTENT);

    let delivered = pull(&client, &base, "NA0642_SLOT_FRESH_ROUTE", 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0642_SLOT_FRESH_ID");

    handle.abort();
}
