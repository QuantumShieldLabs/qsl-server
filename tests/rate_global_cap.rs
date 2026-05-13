use qsl_server::{app, AppState, Limits, ResourceControls};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
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
async fn global_route_cap_rejects_new_routes_without_mutating_existing_routes() {
    let limits = Limits::new(128, 4).unwrap();
    let controls = ResourceControls::new(1, 4, 0).unwrap();
    let (base, handle) = spawn_server_with_auth(limits, controls, None).await;
    let client = reqwest::Client::new();

    let accepted = push(
        &client,
        &base,
        "NA0280_ROUTE_CAP_ACCEPTED",
        None,
        Some("NA0280_ROUTE_CAP_ACCEPTED_ID"),
        b"accepted".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let capped = push(
        &client,
        &base,
        "NA0280_ROUTE_CAP_REJECTED",
        None,
        Some("NA0280_ROUTE_CAP_REJECTED_ID"),
        b"rejected".to_vec(),
    )
    .await;
    assert_eq!(capped.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        capped.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_ROUTE_CAP"
    );

    let rejected_route = pull(&client, &base, "NA0280_ROUTE_CAP_REJECTED", None, 1).await;
    assert_eq!(rejected_route.status(), ReqStatus::NO_CONTENT);

    let delivered = pull(&client, &base, "NA0280_ROUTE_CAP_ACCEPTED", None, 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0280_ROUTE_CAP_ACCEPTED_ID");
    assert_eq!(body.items[0].data.as_slice(), b"accepted");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_pull_does_not_create_route_slot() {
    let limits = Limits::new(128, 4).unwrap();
    let controls = ResourceControls::new(1, 4, 0).unwrap();
    let (base, handle) = spawn_server_with_auth(limits, controls, None).await;
    let client = reqwest::Client::new();

    let unknown = pull(&client, &base, "NA0280_UNKNOWN_PULL", None, 1).await;
    assert_eq!(unknown.status(), ReqStatus::NO_CONTENT);

    let accepted = push(
        &client,
        &base,
        "NA0280_AFTER_UNKNOWN_PULL",
        None,
        Some("NA0280_AFTER_UNKNOWN_PULL_ID"),
        b"accepted-after-unknown".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let delivered = pull(&client, &base, "NA0280_AFTER_UNKNOWN_PULL", None, 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0280_AFTER_UNKNOWN_PULL_ID");
    assert_eq!(body.items[0].data.as_slice(), b"accepted-after-unknown");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn draining_empty_route_releases_global_slot() {
    let limits = Limits::new(128, 4).unwrap();
    let controls = ResourceControls::new(1, 4, 0).unwrap();
    let (base, handle) = spawn_server_with_auth(limits, controls, None).await;
    let client = reqwest::Client::new();

    let first = push(
        &client,
        &base,
        "NA0280_DRAIN_ROUTE_A",
        None,
        Some("NA0280_DRAIN_A_ID"),
        b"a".to_vec(),
    )
    .await;
    assert_eq!(first.status(), ReqStatus::OK);

    let drained = pull(&client, &base, "NA0280_DRAIN_ROUTE_A", None, 1).await;
    assert_eq!(drained.status(), ReqStatus::OK);

    let second = push(
        &client,
        &base,
        "NA0280_DRAIN_ROUTE_B",
        None,
        Some("NA0280_DRAIN_B_ID"),
        b"b".to_vec(),
    )
    .await;
    assert_eq!(second.status(), ReqStatus::OK);

    let delivered = pull(&client, &base, "NA0280_DRAIN_ROUTE_B", None, 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0280_DRAIN_B_ID");
    assert_eq!(body.items[0].data.as_slice(), b"b");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn rate_limit_rejects_without_enqueueing() {
    let limits = Limits::new(128, 4).unwrap();
    let controls = ResourceControls::new(4, 1, 0).unwrap();
    let (base, handle) = spawn_server_with_auth(limits, controls, None).await;
    let client = reqwest::Client::new();

    let accepted = push(
        &client,
        &base,
        "NA0280_RATE_ROUTE",
        None,
        Some("NA0280_RATE_ACCEPTED_ID"),
        b"accepted".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let limited = push(
        &client,
        &base,
        "NA0280_RATE_ROUTE",
        None,
        Some("NA0280_RATE_REJECTED_ID"),
        b"rejected".to_vec(),
    )
    .await;
    assert_eq!(limited.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        limited.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_RATE_LIMITED"
    );

    let delivered = pull(&client, &base, "NA0280_RATE_ROUTE", None, 10).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0280_RATE_ACCEPTED_ID");
    assert_eq!(body.items[0].data.as_slice(), b"accepted");

    let empty = pull(&client, &base, "NA0280_RATE_ROUTE", None, 1).await;
    assert_eq!(empty.status(), ReqStatus::NO_CONTENT);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn wrong_auth_and_oversize_do_not_consume_route_or_rate_state() {
    let auth_token = "NA0280_AUTH_TOKEN";
    let wrong_auth = "NA0280_WRONG_AUTH";
    let limits = Limits::new(4, 4).unwrap();
    let controls = ResourceControls::new(1, 1, 0).unwrap();
    let (base, handle) = spawn_server_with_auth(limits, controls, Some(auth_token)).await;
    let client = reqwest::Client::new();

    let auth_reject = push(
        &client,
        &base,
        "NA0280_WRONG_AUTH_ROUTE",
        Some(wrong_auth),
        Some("NA0280_WRONG_AUTH_ID"),
        b"fits".to_vec(),
    )
    .await;
    assert_eq!(auth_reject.status(), ReqStatus::UNAUTHORIZED);
    assert_eq!(
        auth_reject.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_UNAUTHORIZED"
    );

    let oversize = push(
        &client,
        &base,
        "NA0280_OVERSIZE_ROUTE",
        Some(auth_token),
        Some("NA0280_OVERSIZE_ID"),
        b"too-large".to_vec(),
    )
    .await;
    assert_eq!(oversize.status(), ReqStatus::PAYLOAD_TOO_LARGE);
    assert_eq!(
        oversize.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_TOO_LARGE"
    );

    let accepted = push(
        &client,
        &base,
        "NA0280_AUTH_GOOD_ROUTE",
        Some(auth_token),
        Some("NA0280_AUTH_GOOD_ID"),
        b"good".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let capped = push(
        &client,
        &base,
        "NA0280_UNRELATED_ROUTE",
        Some(auth_token),
        Some("NA0280_UNRELATED_ID"),
        b"deny".to_vec(),
    )
    .await;
    assert_eq!(capped.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        capped.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_ROUTE_CAP"
    );

    let delivered = pull(
        &client,
        &base,
        "NA0280_AUTH_GOOD_ROUTE",
        Some(auth_token),
        1,
    )
    .await;
    assert_eq!(delivered.status(), ReqStatus::OK);

    let wrong_auth_route_reusable = push(
        &client,
        &base,
        "NA0280_WRONG_AUTH_ROUTE",
        Some(auth_token),
        Some("NA0280_WRONG_AUTH_ROUTE_LATER_ID"),
        b"fits".to_vec(),
    )
    .await;
    assert_eq!(wrong_auth_route_reusable.status(), ReqStatus::OK);
    let drain_wrong_auth = pull(
        &client,
        &base,
        "NA0280_WRONG_AUTH_ROUTE",
        Some(auth_token),
        1,
    )
    .await;
    assert_eq!(drain_wrong_auth.status(), ReqStatus::OK);

    let oversize_route_reusable = push(
        &client,
        &base,
        "NA0280_OVERSIZE_ROUTE",
        Some(auth_token),
        Some("NA0280_OVERSIZE_ROUTE_LATER_ID"),
        b"fits".to_vec(),
    )
    .await;
    assert_eq!(oversize_route_reusable.status(), ReqStatus::OK);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn existing_queue_overload_still_returns_err_overloaded() {
    let limits = Limits::new(128, 1).unwrap();
    let controls = ResourceControls::new(4, 4, 0).unwrap();
    let (base, handle) = spawn_server_with_auth(limits, controls, None).await;
    let client = reqwest::Client::new();

    let accepted = push(
        &client,
        &base,
        "NA0280_OVERLOAD_ROUTE",
        None,
        Some("NA0280_OVERLOAD_ACCEPTED_ID"),
        b"a".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let overloaded = push(
        &client,
        &base,
        "NA0280_OVERLOAD_ROUTE",
        None,
        Some("NA0280_OVERLOAD_REJECTED_ID"),
        b"b".to_vec(),
    )
    .await;
    assert_eq!(overloaded.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        overloaded.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_OVERLOADED"
    );

    let delivered = pull(&client, &base, "NA0280_OVERLOAD_ROUTE", None, 2).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0280_OVERLOAD_ACCEPTED_ID");
    assert_eq!(body.items[0].data.as_slice(), b"a");

    handle.abort();
}
