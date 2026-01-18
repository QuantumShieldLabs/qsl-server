use qsl_server::{app, AppState, Limits};
use reqwest::StatusCode as ReqStatus;
use tokio::net::TcpListener;

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

#[tokio::test]
async fn push_then_pull_roundtrip() {
    let (base, handle) = spawn_server(Limits {
        max_body_bytes: 1024 * 1024,
        max_queue_depth: 8,
    })
    .await;

    let client = reqwest::Client::new();
    let payload = b"opaque-bytes".to_vec();
    let push = client
        .post(format!("{}/v1/push/test", base))
        .body(payload.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(push.status(), ReqStatus::OK);

    let pull = client
        .get(format!("{}/v1/pull/test", base))
        .send()
        .await
        .unwrap();
    assert_eq!(pull.status(), ReqStatus::OK);
    let body = pull.bytes().await.unwrap();
    assert_eq!(body.as_ref(), payload.as_slice());

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
    let pull = client
        .get(format!("{}/v1/pull/empty", base))
        .send()
        .await
        .unwrap();
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
    let push = client
        .post(format!("{}/v1/push/oversize", base))
        .body(vec![0u8; 5])
        .send()
        .await
        .unwrap();
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
    let r1 = client
        .post(format!("{}/v1/push/qfull", base))
        .body(b"a".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), ReqStatus::OK);

    let r2 = client
        .post(format!("{}/v1/push/qfull", base))
        .body(b"b".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), ReqStatus::TOO_MANY_REQUESTS);
    let body = r2.text().await.unwrap();
    assert_eq!(body, "ERR_QUEUE_FULL");
    handle.abort();
}
