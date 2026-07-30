use qsl_server::{app, AppState, Limits};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use std::process::Command;
use tokio::net::TcpListener;
use tracing::subscriber::set_default;

mod common;
use common::{await_logs, capture, install_permissive_global_once};

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";

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
    body: impl Into<Vec<u8>>,
) -> reqwest::Response {
    let mut request = client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .body(body.into());
    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {token}"));
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
async fn auth_rejects_are_401_and_do_not_mutate() {
    let (base, handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 1024,
            max_queue_depth: 8,
        },
        Some("required-relay-token"),
    )
    .await;
    let client = reqwest::Client::new();

    let missing = client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, "auth-route")
        .body(b"missing-auth".to_vec())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(missing.status(), ReqStatus::UNAUTHORIZED);
    assert_eq!(
        missing.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_UNAUTHORIZED"
    );

    let wrong = push(
        &client,
        &base,
        "auth-route",
        Some("wrong-relay-token"),
        b"wrong-auth".to_vec(),
    )
    .await;
    assert_eq!(wrong.status(), ReqStatus::UNAUTHORIZED);
    assert_eq!(
        wrong.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_UNAUTHORIZED"
    );

    let empty_after_rejects = pull(
        &client,
        &base,
        "auth-route",
        Some("required-relay-token"),
        1,
    )
    .await;
    assert_eq!(empty_after_rejects.status(), ReqStatus::NO_CONTENT);

    let accepted = push(
        &client,
        &base,
        "auth-route",
        Some("required-relay-token"),
        b"accepted".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let delivered = pull(
        &client,
        &base,
        "auth-route",
        Some("required-relay-token"),
        1,
    )
    .await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].data.as_slice(), b"accepted");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn route_token_rejects_and_unknown_routes_are_deterministic() {
    let (base, handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 1024,
            max_queue_depth: 8,
        },
        None,
    )
    .await;
    let client = reqwest::Client::new();

    let missing = client
        .post(format!("{base}/v1/push"))
        .body(b"missing-route".to_vec())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(missing.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(
        missing.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_MISSING_ROUTE_TOKEN"
    );

    let empty = client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, "   ")
        .body(b"empty-route".to_vec())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(empty.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(
        empty.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_MISSING_ROUTE_TOKEN"
    );

    let missing_pull = client
        .get(format!("{base}/v1/pull?max=1"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(missing_pull.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(
        missing_pull.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_MISSING_ROUTE_TOKEN"
    );

    let unknown = pull(&client, &base, "unknown-route", None, 1).await;
    assert_eq!(unknown.status(), ReqStatus::NO_CONTENT);

    let accepted = push(&client, &base, "known-route", None, b"known".to_vec()).await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let wrong_route = pull(&client, &base, "different-route", None, 1).await;
    assert_eq!(wrong_route.status(), ReqStatus::NO_CONTENT);

    let original_route = pull(&client, &base, "known-route", None, 1).await;
    assert_eq!(original_route.status(), ReqStatus::OK);
    let body: PullResp = original_route
        .json()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].data.as_slice(), b"known");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn rejected_pushes_do_not_mutate_for_size_or_depth() {
    let (base, handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 4,
            max_queue_depth: 1,
        },
        None,
    )
    .await;
    let client = reqwest::Client::new();

    let oversize = push(&client, &base, "oversize-route", None, vec![0u8; 5]).await;
    assert_eq!(oversize.status(), ReqStatus::PAYLOAD_TOO_LARGE);
    assert_eq!(
        oversize.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_TOO_LARGE"
    );
    let empty_after_oversize = pull(&client, &base, "oversize-route", None, 1).await;
    assert_eq!(empty_after_oversize.status(), ReqStatus::NO_CONTENT);

    let first = push(&client, &base, "depth-route", None, b"a".to_vec()).await;
    assert_eq!(first.status(), ReqStatus::OK);
    let second = push(&client, &base, "depth-route", None, b"b".to_vec()).await;
    assert_eq!(second.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        second.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_OVERLOADED"
    );

    let delivered = pull(&client, &base, "depth-route", None, 999).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].data.as_slice(), b"a");
    assert!(!body.items[0].id.is_empty());

    let empty_after_delivery = pull(&client, &base, "depth-route", None, 1).await;
    assert_eq!(empty_after_delivery.status(), ReqStatus::NO_CONTENT);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn pull_json_delete_and_bad_max_behavior_are_deterministic() {
    let (base, handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 1024,
            max_queue_depth: 8,
        },
        None,
    )
    .await;
    let client = reqwest::Client::new();

    let empty = pull(&client, &base, "pull-route", None, 1).await;
    assert_eq!(empty.status(), ReqStatus::NO_CONTENT);

    assert_eq!(
        push(&client, &base, "pull-route", None, b"one".to_vec())
            .await
            .status(),
        ReqStatus::OK
    );
    assert_eq!(
        push(&client, &base, "pull-route", None, b"two".to_vec())
            .await
            .status(),
        ReqStatus::OK
    );

    let delivered = pull(&client, &base, "pull-route", None, 2).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 2);
    assert_eq!(body.items[0].data.as_slice(), b"one");
    assert_eq!(body.items[1].data.as_slice(), b"two");

    let empty_after_delivery = pull(&client, &base, "pull-route", None, 1).await;
    assert_eq!(empty_after_delivery.status(), ReqStatus::NO_CONTENT);

    assert_eq!(
        push(&client, &base, "bad-max-route", None, b"survives".to_vec())
            .await
            .status(),
        ReqStatus::OK
    );
    let bad_max = pull(&client, &base, "bad-max-route", None, 0).await;
    assert_eq!(bad_max.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(
        bad_max.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_BAD_MAX"
    );
    let after_bad_max = pull(&client, &base, "bad-max-route", None, 1).await;
    assert_eq!(after_bad_max.status(), ReqStatus::OK);
    let body: PullResp = after_bad_max.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].data.as_slice(), b"survives");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_routes_404_without_mutating_or_consuming() {
    let (base, handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 1024,
            max_queue_depth: 8,
        },
        None,
    )
    .await;
    let client = reqwest::Client::new();

    let legacy_push = client
        .post(format!("{base}/v1/push/legacy-route-token"))
        .header(ROUTE_TOKEN_HEADER, "legacy-route-token")
        .body(b"legacy-body".to_vec())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(legacy_push.status(), ReqStatus::NOT_FOUND);
    let empty_after_legacy_push = pull(&client, &base, "legacy-route-token", None, 1).await;
    assert_eq!(empty_after_legacy_push.status(), ReqStatus::NO_CONTENT);

    assert_eq!(
        push(
            &client,
            &base,
            "legacy-route-token",
            None,
            b"canonical".to_vec()
        )
        .await
        .status(),
        ReqStatus::OK
    );
    let legacy_pull = client
        .get(format!("{base}/v1/pull/legacy-route-token?max=1"))
        .header(ROUTE_TOKEN_HEADER, "legacy-route-token")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(legacy_pull.status(), ReqStatus::NOT_FOUND);

    let canonical_pull = pull(&client, &base, "legacy-route-token", None, 1).await;
    assert_eq!(canonical_pull.status(), ReqStatus::OK);
    let body: PullResp = canonical_pull
        .json()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].data.as_slice(), b"canonical");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn logs_do_not_leak_route_auth_or_payload_on_success_or_rejects() {
    install_permissive_global_once();
    let (buf, writer) = capture();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = set_default(subscriber);

    let route_token = "NA0273_ROUTE_TOKEN_SENTINEL";
    let auth_token = "NA0273_AUTH_TOKEN_SENTINEL";
    let wrong_auth = "NA0273_WRONG_AUTH_SENTINEL";
    let payload = b"NA0273_PAYLOAD_SENTINEL".to_vec();
    let overload_payload = b"NA0273_OVERLOAD_PAYLOAD_SENTINEL".to_vec();
    let reject_payload = b"NA0273_REJECT_PAYLOAD_SENTINEL".to_vec();

    let (base, handle) = spawn_server_with_auth(
        Limits {
            max_body_bytes: 1024,
            max_queue_depth: 1,
        },
        Some(auth_token),
    )
    .await;
    let client = reqwest::Client::new();

    let accepted = push(&client, &base, route_token, Some(auth_token), payload).await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let overloaded = push(
        &client,
        &base,
        route_token,
        Some(auth_token),
        overload_payload,
    )
    .await;
    assert_eq!(overloaded.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        overloaded.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_OVERLOADED"
    );

    let auth_reject = push(
        &client,
        &base,
        route_token,
        Some(wrong_auth),
        reject_payload,
    )
    .await;
    assert_eq!(auth_reject.status(), ReqStatus::UNAUTHORIZED);
    assert_eq!(
        auth_reject.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_UNAUTHORIZED"
    );

    let delivered = pull(&client, &base, route_token, Some(auth_token), 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);

    // NA-0687: await the relay's own log lines BEFORE aborting the task. abort()
    // guarantees a not-yet-emitted line is never emitted, so a wait placed after it
    // could not succeed. The single `yield_now()` this replaces gave the server task
    // exactly one scheduling opportunity -- a nudge, not a synchronisation.
    let logs = await_logs(&buf, &["push channel_id=", "event=overloaded"]).await;
    handle.abort();

    assert!(logs.contains("push channel_id="));
    assert!(logs.contains("event=overloaded"));

    for forbidden in [
        route_token,
        auth_token,
        wrong_auth,
        "Authorization",
        "Bearer",
        "NA0273_PAYLOAD_SENTINEL",
        "NA0273_OVERLOAD_PAYLOAD_SENTINEL",
        "NA0273_REJECT_PAYLOAD_SENTINEL",
    ] {
        assert!(!logs.contains(forbidden), "logs leaked {forbidden}");
    }
}

#[test]
fn startup_config_rejects_invalid_port() {
    let invalid_port = Command::new(env!("CARGO_BIN_EXE_qsl-server"))
        .env_clear()
        .env("RUST_LOG", "error")
        .env("PORT", "not-a-port")
        .output()
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(!invalid_port.status.success());
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&invalid_port.stdout),
        String::from_utf8_lossy(&invalid_port.stderr)
    );
    assert!(output.contains("ERR_INVALID_ENV_PORT"));
}
