use qsl_server::{app, AppState, Limits};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use tokio::net::TcpListener;

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";

async fn spawn_server(limits: Limits) -> (String, tokio::task::JoinHandle<()>) {
    let state = AppState::new(limits);
    let app = app(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{}", addr), handle)
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

async fn canonical_push(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    body: Vec<u8>,
) -> reqwest::Response {
    client
        .post(format!("{}/v1/push", base))
        .header(ROUTE_TOKEN_HEADER, token)
        .body(body)
        .send()
        .await
        .unwrap()
}

async fn canonical_pull(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    max: usize,
) -> reqwest::Response {
    client
        .get(format!("{}/v1/pull?max={}", base, max))
        .header(ROUTE_TOKEN_HEADER, token)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn push_then_pull_roundtrip() {
    let (base, handle) = spawn_server(Limits {
        max_body_bytes: 1024 * 1024,
        max_queue_depth: 8,
    })
    .await;

    let client = reqwest::Client::new();
    let payload = b"opaque-bytes".to_vec();
    let push = canonical_push(&client, &base, "test", payload.clone()).await;
    assert_eq!(push.status(), ReqStatus::OK);

    let pull = canonical_pull(&client, &base, "test", 1).await;
    assert_eq!(pull.status(), ReqStatus::OK);
    let body: PullResp = pull.json().await.unwrap();
    assert_eq!(body.items.len(), 1);
    assert!(!body.items[0].id.is_empty());
    assert_eq!(body.items[0].data.as_slice(), payload.as_slice());

    handle.abort();
}

#[tokio::test]
async fn pull_empty_returns_204() {
    let (base, handle) = spawn_server(Limits {
        max_body_bytes: 1024 * 1024,
        max_queue_depth: 8,
    })
    .await;
    let client = reqwest::Client::new();
    let pull = canonical_pull(&client, &base, "empty", 1).await;
    assert_eq!(pull.status(), ReqStatus::NO_CONTENT);
    handle.abort();
}

#[tokio::test]
async fn oversize_returns_413() {
    let (base, handle) = spawn_server(Limits {
        max_body_bytes: 4,
        max_queue_depth: 8,
    })
    .await;
    let client = reqwest::Client::new();
    let push = canonical_push(&client, &base, "oversize", vec![0u8; 5]).await;
    assert_eq!(push.status(), ReqStatus::PAYLOAD_TOO_LARGE);
    let body = push.text().await.unwrap();
    assert_eq!(body, "ERR_TOO_LARGE");
    handle.abort();
}

#[tokio::test]
async fn queue_full_returns_429() {
    let (base, handle) = spawn_server(Limits {
        max_body_bytes: 1024 * 1024,
        max_queue_depth: 1,
    })
    .await;
    let client = reqwest::Client::new();
    let r1 = canonical_push(&client, &base, "qfull", b"a".to_vec()).await;
    assert_eq!(r1.status(), ReqStatus::OK);

    let r2 = canonical_push(&client, &base, "qfull", b"b".to_vec()).await;
    assert_eq!(r2.status(), ReqStatus::TOO_MANY_REQUESTS);
    let body = r2.text().await.unwrap();
    assert_eq!(body, "ERR_OVERLOADED");
    handle.abort();
}

#[tokio::test]
async fn legacy_path_roundtrip_still_works_during_compatibility_window() {
    let (base, handle) = spawn_server(Limits {
        max_body_bytes: 1024 * 1024,
        max_queue_depth: 8,
    })
    .await;
    let client = reqwest::Client::new();
    let payload = b"legacy-compat".to_vec();
    let push = client
        .post(format!("{}/v1/push/legacy", base))
        .body(payload.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(push.status(), ReqStatus::OK);

    let pull = client
        .get(format!("{}/v1/pull/legacy?max=1", base))
        .send()
        .await
        .unwrap();
    assert_eq!(pull.status(), ReqStatus::OK);
    let body: PullResp = pull.json().await.unwrap();
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].data.as_slice(), payload.as_slice());
    handle.abort();
}

#[tokio::test]
async fn legacy_path_header_mismatch_rejects_without_mutation() {
    let (base, handle) = spawn_server(Limits {
        max_body_bytes: 1024 * 1024,
        max_queue_depth: 8,
    })
    .await;
    let client = reqwest::Client::new();
    let push = client
        .post(format!("{}/v1/push/legacy-token", base))
        .header(ROUTE_TOKEN_HEADER, "other-token")
        .body(b"mismatch".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(push.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(push.text().await.unwrap(), "ERR_ROUTE_TOKEN_MISMATCH");

    let pull = client
        .get(format!("{}/v1/pull/legacy-token?max=1", base))
        .send()
        .await
        .unwrap();
    assert_eq!(pull.status(), ReqStatus::NO_CONTENT);
    handle.abort();
}

#[tokio::test]
async fn legacy_path_header_equal_is_accepted() {
    let (base, handle) = spawn_server(Limits {
        max_body_bytes: 1024 * 1024,
        max_queue_depth: 8,
    })
    .await;
    let client = reqwest::Client::new();
    let push = client
        .post(format!("{}/v1/push/legacy-equal", base))
        .header(ROUTE_TOKEN_HEADER, "legacy-equal")
        .body(b"same".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(push.status(), ReqStatus::OK);

    let pull = client
        .get(format!("{}/v1/pull/legacy-equal?max=1", base))
        .header(ROUTE_TOKEN_HEADER, "legacy-equal")
        .send()
        .await
        .unwrap();
    assert_eq!(pull.status(), ReqStatus::OK);
    let body: PullResp = pull.json().await.unwrap();
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].data.as_slice(), b"same");
    handle.abort();
}

#[tokio::test]
async fn canonical_missing_header_rejects_without_mutation() {
    let (base, handle) = spawn_server(Limits {
        max_body_bytes: 1024 * 1024,
        max_queue_depth: 8,
    })
    .await;
    let client = reqwest::Client::new();
    let push = client
        .post(format!("{}/v1/push", base))
        .body(b"missing".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(push.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(push.text().await.unwrap(), "ERR_MISSING_ROUTE_TOKEN");

    let pull = canonical_pull(&client, &base, "missing", 1).await;
    assert_eq!(pull.status(), ReqStatus::NO_CONTENT);
    handle.abort();
}

#[tokio::test]
async fn canonical_empty_header_rejects_without_mutation() {
    let (base, handle) = spawn_server(Limits {
        max_body_bytes: 1024 * 1024,
        max_queue_depth: 8,
    })
    .await;
    let client = reqwest::Client::new();
    let push = client
        .post(format!("{}/v1/push", base))
        .header(ROUTE_TOKEN_HEADER, "   ")
        .body(b"empty".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(push.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(push.text().await.unwrap(), "ERR_MISSING_ROUTE_TOKEN");

    let pull = canonical_pull(&client, &base, "empty", 1).await;
    assert_eq!(pull.status(), ReqStatus::NO_CONTENT);
    handle.abort();
}
