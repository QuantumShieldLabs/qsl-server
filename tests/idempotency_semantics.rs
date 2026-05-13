use qsl_server::{app, AppState, Limits};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use tokio::net::TcpListener;

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const MSG_ID_HEADER: &str = "X-Msg-Id";

#[derive(Deserialize)]
struct PostResp {
    id: String,
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
async fn duplicate_x_msg_id_pushes_enqueue_fifo_items_and_delete_on_pull() {
    let (base, handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 1024,
            max_queue_depth: 8,
        },
        None,
    )
    .await;
    let client = reqwest::Client::new();
    let msg_id = "NA0275_DUPLICATE_MESSAGE_ID";

    let first = push(
        &client,
        &base,
        "duplicate-route",
        None,
        Some(msg_id),
        b"first".to_vec(),
    )
    .await;
    assert_eq!(first.status(), ReqStatus::OK);
    let first_body: PostResp = first.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(first_body.id, msg_id);

    let second = push(
        &client,
        &base,
        "duplicate-route",
        None,
        Some(msg_id),
        b"second".to_vec(),
    )
    .await;
    assert_eq!(second.status(), ReqStatus::OK);
    let second_body: PostResp = second.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(second_body.id, msg_id);

    let delivered = pull(&client, &base, "duplicate-route", None, 2).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 2);
    assert_eq!(body.items[0].id, msg_id);
    assert_eq!(body.items[0].data.as_slice(), b"first");
    assert_eq!(body.items[1].id, msg_id);
    assert_eq!(body.items[1].data.as_slice(), b"second");

    let empty_after_delete = pull(&client, &base, "duplicate-route", None, 1).await;
    assert_eq!(empty_after_delete.status(), ReqStatus::NO_CONTENT);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn mixed_supplied_and_auto_ids_preserve_order_and_payloads() {
    let (base, handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 1024,
            max_queue_depth: 8,
        },
        None,
    )
    .await;
    let client = reqwest::Client::new();
    let supplied = "NA0275_MIXED_SUPPLIED_ID";

    let supplied_one = push(
        &client,
        &base,
        "mixed-route",
        None,
        Some(supplied),
        b"supplied-one".to_vec(),
    )
    .await;
    assert_eq!(supplied_one.status(), ReqStatus::OK);
    let supplied_one_body: PostResp = supplied_one.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(supplied_one_body.id, supplied);

    let auto = push(&client, &base, "mixed-route", None, None, b"auto".to_vec()).await;
    assert_eq!(auto.status(), ReqStatus::OK);
    let auto_body: PostResp = auto.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert!(!auto_body.id.is_empty());
    assert_ne!(auto_body.id, supplied);

    let supplied_two = push(
        &client,
        &base,
        "mixed-route",
        None,
        Some(supplied),
        b"supplied-two".to_vec(),
    )
    .await;
    assert_eq!(supplied_two.status(), ReqStatus::OK);

    let delivered = pull(&client, &base, "mixed-route", None, 3).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 3);
    assert_eq!(body.items[0].id, supplied);
    assert_eq!(body.items[0].data.as_slice(), b"supplied-one");
    assert_eq!(body.items[1].id, auto_body.id);
    assert_eq!(body.items[1].data.as_slice(), b"auto");
    assert_eq!(body.items[2].id, supplied);
    assert_eq!(body.items[2].data.as_slice(), b"supplied-two");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn rejected_duplicate_id_attempts_do_not_mutate_queues() {
    let msg_id = "NA0275_REJECTED_DUPLICATE_ID";

    let (auth_base, auth_handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 1024,
            max_queue_depth: 8,
        },
        Some("NA0275_REQUIRED_AUTH_TOKEN"),
    )
    .await;
    let client = reqwest::Client::new();
    let accepted = push(
        &client,
        &auth_base,
        "auth-route",
        Some("NA0275_REQUIRED_AUTH_TOKEN"),
        Some(msg_id),
        b"accepted".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let auth_reject = push(
        &client,
        &auth_base,
        "auth-route",
        Some("wrong-auth-token"),
        Some(msg_id),
        b"must-not-enqueue".to_vec(),
    )
    .await;
    assert_eq!(auth_reject.status(), ReqStatus::UNAUTHORIZED);
    assert_eq!(
        auth_reject.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_UNAUTHORIZED"
    );

    let delivered = pull(
        &client,
        &auth_base,
        "auth-route",
        Some("NA0275_REQUIRED_AUTH_TOKEN"),
        2,
    )
    .await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, msg_id);
    assert_eq!(body.items[0].data.as_slice(), b"accepted");
    auth_handle.abort();

    let (oversize_base, oversize_handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 4,
            max_queue_depth: 8,
        },
        None,
    )
    .await;
    let accepted = push(
        &client,
        &oversize_base,
        "oversize-route",
        None,
        Some(msg_id),
        b"fits".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);
    let oversize = push(
        &client,
        &oversize_base,
        "oversize-route",
        None,
        Some(msg_id),
        b"too-large".to_vec(),
    )
    .await;
    assert_eq!(oversize.status(), ReqStatus::PAYLOAD_TOO_LARGE);
    assert_eq!(
        oversize.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_TOO_LARGE"
    );
    let delivered = pull(&client, &oversize_base, "oversize-route", None, 2).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, msg_id);
    assert_eq!(body.items[0].data.as_slice(), b"fits");
    oversize_handle.abort();

    let (depth_base, depth_handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 1024,
            max_queue_depth: 1,
        },
        None,
    )
    .await;
    let accepted = push(
        &client,
        &depth_base,
        "depth-route",
        None,
        Some(msg_id),
        b"first".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);
    let full = push(
        &client,
        &depth_base,
        "depth-route",
        None,
        Some(msg_id),
        b"second".to_vec(),
    )
    .await;
    assert_eq!(full.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        full.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_OVERLOADED"
    );
    let delivered = pull(&client, &depth_base, "depth-route", None, 2).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, msg_id);
    assert_eq!(body.items[0].data.as_slice(), b"first");
    depth_handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn blank_x_msg_id_is_treated_as_absent_without_panic() {
    let (base, handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 1024,
            max_queue_depth: 8,
        },
        None,
    )
    .await;
    let client = reqwest::Client::new();

    let accepted = push(
        &client,
        &base,
        "blank-id-route",
        None,
        Some("   "),
        b"blank-id".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);
    let accepted_body: PostResp = accepted.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert!(!accepted_body.id.is_empty());
    assert_ne!(accepted_body.id.trim(), "");

    let delivered = pull(&client, &base, "blank-id-route", None, 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, accepted_body.id);
    assert_eq!(body.items[0].data.as_slice(), b"blank-id");

    handle.abort();
}
