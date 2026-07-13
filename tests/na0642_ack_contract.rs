// NA-0642 acknowledged-pull contract (design-lock option B):
// - GET /v1/pull?ack=lease returns messages WITHOUT deleting; each returned
//   message is leased (in-flight) until now + PULL_LEASE_SECS.
// - POST /v1/pull/ack {"ids":[...]} deletes ONLY leased copies; unleased
//   duplicates (NA-0275 contract) survive.
// - Un-acked leased messages reappear after the lease expires.
// - Legacy pulls (no ack param) keep delete-on-deliver and never see messages
//   another pull holds under a live lease.

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

#[derive(Deserialize)]
struct AckResp {
    acked: usize,
}

async fn spawn_server(lease_secs: usize) -> (String, tokio::task::JoinHandle<()>) {
    let store = StoreConfig {
        pull_lease_secs: lease_secs,
        ..StoreConfig::default()
    };
    let state = AppState::new_with_auth_controls_and_store(
        Limits::new(1024 * 1024, 16).unwrap(),
        ResourceControls::new(8, 16, 16).unwrap(),
        None,
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
    (format!("http://{addr}"), handle)
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

async fn pull_legacy(
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

async fn pull_lease(
    client: &reqwest::Client,
    base: &str,
    route_token: &str,
    max: usize,
) -> reqwest::Response {
    client
        .get(format!("{base}/v1/pull?max={max}&ack=lease"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
}

async fn ack(
    client: &reqwest::Client,
    base: &str,
    route_token: &str,
    ids: &[&str],
) -> reqwest::Response {
    client
        .post(format!("{base}/v1/pull/ack"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .json(&serde_json::json!({ "ids": ids }))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
}

#[tokio::test(flavor = "current_thread")]
async fn lease_pull_does_not_delete_until_ack() {
    let (base, handle) = spawn_server(60).await;
    let client = reqwest::Client::new();
    let route = "NA0642_ACK_BASIC";

    let accepted = push(
        &client,
        &base,
        route,
        Some("NA0642_ACK_MSG_1"),
        b"m1".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let leased = pull_lease(&client, &base, route, 4).await;
    assert_eq!(leased.status(), ReqStatus::OK);
    let body: PullResp = leased.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0642_ACK_MSG_1");
    assert_eq!(body.items[0].data.as_slice(), b"m1");

    // In-flight: invisible to both pull modes while the lease is live.
    let release = pull_lease(&client, &base, route, 4).await;
    assert_eq!(release.status(), ReqStatus::NO_CONTENT);
    let legacy = pull_legacy(&client, &base, route, 4).await;
    assert_eq!(legacy.status(), ReqStatus::NO_CONTENT);

    let acked = ack(&client, &base, route, &["NA0642_ACK_MSG_1"]).await;
    assert_eq!(acked.status(), ReqStatus::OK);
    let acked: AckResp = acked.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(acked.acked, 1);

    let after = pull_lease(&client, &base, route, 4).await;
    assert_eq!(after.status(), ReqStatus::NO_CONTENT);

    // Idempotent: re-acking a deleted id succeeds with acked=0.
    let reack = ack(&client, &base, route, &["NA0642_ACK_MSG_1"]).await;
    assert_eq!(reack.status(), ReqStatus::OK);
    let reack: AckResp = reack.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reack.acked, 0);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn unacked_lease_reappears_after_expiry() {
    let (base, handle) = spawn_server(1).await;
    let client = reqwest::Client::new();
    let route = "NA0642_ACK_REDELIVERY";

    let accepted = push(
        &client,
        &base,
        route,
        Some("NA0642_REDELIVER_1"),
        b"r1".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let leased = pull_lease(&client, &base, route, 1).await;
    assert_eq!(leased.status(), ReqStatus::OK);

    // No ack (the puller "crashed"); the message must come back.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let redelivered = pull_lease(&client, &base, route, 1).await;
    assert_eq!(redelivered.status(), ReqStatus::OK);
    let body: PullResp = redelivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0642_REDELIVER_1");
    assert_eq!(body.items[0].data.as_slice(), b"r1");

    let acked = ack(&client, &base, route, &["NA0642_REDELIVER_1"]).await;
    assert_eq!(acked.status(), ReqStatus::OK);
    let acked: AckResp = acked.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(acked.acked, 1);
    let after = pull_legacy(&client, &base, route, 1).await;
    assert_eq!(after.status(), ReqStatus::NO_CONTENT);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn ack_only_deletes_leased_copies() {
    // Never-pulled messages cannot be acked away; with duplicate msg_ids
    // (NA-0275: both copies enqueue) only the delivered copy dies.
    let (base, handle) = spawn_server(60).await;
    let client = reqwest::Client::new();
    let route = "NA0642_ACK_LEASED_ONLY";

    let first = push(
        &client,
        &base,
        route,
        Some("NA0642_DUP_ID"),
        b"copy-1".to_vec(),
    )
    .await;
    assert_eq!(first.status(), ReqStatus::OK);
    let second = push(
        &client,
        &base,
        route,
        Some("NA0642_DUP_ID"),
        b"copy-2".to_vec(),
    )
    .await;
    assert_eq!(second.status(), ReqStatus::OK);

    // Ack before any pull: nothing is leased, nothing may be deleted.
    let premature = ack(&client, &base, route, &["NA0642_DUP_ID"]).await;
    assert_eq!(premature.status(), ReqStatus::OK);
    let premature: AckResp = premature.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(premature.acked, 0);

    // Lease exactly one copy, then ack the id: only the leased copy dies.
    let leased = pull_lease(&client, &base, route, 1).await;
    assert_eq!(leased.status(), ReqStatus::OK);
    let leased_body: PullResp = leased.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(leased_body.items.len(), 1);
    assert_eq!(leased_body.items[0].data.as_slice(), b"copy-1");

    let acked = ack(&client, &base, route, &["NA0642_DUP_ID"]).await;
    assert_eq!(acked.status(), ReqStatus::OK);
    let acked: AckResp = acked.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(acked.acked, 1);

    // The undelivered duplicate survives and is still deliverable.
    let survivor = pull_legacy(&client, &base, route, 2).await;
    assert_eq!(survivor.status(), ReqStatus::OK);
    let body: PullResp = survivor.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0642_DUP_ID");
    assert_eq!(body.items[0].data.as_slice(), b"copy-2");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn ack_and_mode_inputs_fail_closed() {
    let (base, handle) = spawn_server(60).await;
    let client = reqwest::Client::new();
    let route = "NA0642_ACK_INPUTS";

    let bad_mode = client
        .get(format!("{base}/v1/pull?max=1&ack=bogus"))
        .header(ROUTE_TOKEN_HEADER, route)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(bad_mode.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(
        bad_mode.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_BAD_ACK_MODE"
    );

    let empty_ids = ack(&client, &base, route, &[]).await;
    assert_eq!(empty_ids.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(
        empty_ids.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_BAD_ACK_IDS"
    );

    let bad_body = client
        .post(format!("{base}/v1/pull/ack"))
        .header(ROUTE_TOKEN_HEADER, route)
        .body("not-json")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(bad_body.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(
        bad_body.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_BAD_ACK_BODY"
    );

    let missing_route = client
        .post(format!("{base}/v1/pull/ack"))
        .json(&serde_json::json!({ "ids": ["x"] }))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(missing_route.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(
        missing_route.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_MISSING_ROUTE_TOKEN"
    );

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn ack_is_scoped_to_the_route() {
    // An ack on route B must not delete route A's leased message.
    let (base, handle) = spawn_server(60).await;
    let client = reqwest::Client::new();

    let accepted = push(
        &client,
        &base,
        "NA0642_ACK_ROUTE_A",
        Some("NA0642_CROSS_ID"),
        b"a-copy".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);
    let leased = pull_lease(&client, &base, "NA0642_ACK_ROUTE_A", 1).await;
    assert_eq!(leased.status(), ReqStatus::OK);

    let cross = ack(&client, &base, "NA0642_ACK_ROUTE_B", &["NA0642_CROSS_ID"]).await;
    assert_eq!(cross.status(), ReqStatus::OK);
    let cross: AckResp = cross.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(cross.acked, 0);

    // Still deletable by the right route.
    let acked = ack(&client, &base, "NA0642_ACK_ROUTE_A", &["NA0642_CROSS_ID"]).await;
    assert_eq!(acked.status(), ReqStatus::OK);
    let acked: AckResp = acked.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(acked.acked, 1);

    handle.abort();
}
