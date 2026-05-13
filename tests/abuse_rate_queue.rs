use qsl_server::{app, AppState, Limits};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;
use tracing::subscriber::set_default;

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const MSG_ID_HEADER: &str = "X-Msg-Id";

#[derive(Clone)]
struct SharedWriter(Arc<Mutex<Vec<u8>>>);

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut guard = self.0.lock().unwrap_or_else(|e| panic!("{e}"));
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
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
async fn queue_cap_overload_drain_and_route_isolation_are_deterministic() {
    let (base, handle) = spawn_server_with_auth(Limits::new(64, 3).unwrap(), None).await;
    let client = reqwest::Client::new();
    let overloaded_route = "NA0277_ROUTE_QUEUE_CAP_SENTINEL";
    let isolated_route = "NA0277_ROUTE_ISOLATED_SENTINEL";

    for (idx, body) in [b"accepted-0", b"accepted-1", b"accepted-2"]
        .into_iter()
        .enumerate()
    {
        let msg_id = format!("NA0277_ACCEPTED_{idx}");
        let accepted = push(&client, &base, overloaded_route, None, Some(&msg_id), body).await;
        assert_eq!(accepted.status(), ReqStatus::OK);
    }

    for body in [b"overload-0", b"overload-1"] {
        let overloaded = push(&client, &base, overloaded_route, None, None, body).await;
        assert_eq!(overloaded.status(), ReqStatus::TOO_MANY_REQUESTS);
        assert_eq!(
            overloaded.text().await.unwrap_or_else(|e| panic!("{e}")),
            "ERR_OVERLOADED"
        );
    }

    let isolated = push(
        &client,
        &base,
        isolated_route,
        None,
        Some("NA0277_ISOLATED_ACCEPTED"),
        b"isolated-ok".to_vec(),
    )
    .await;
    assert_eq!(isolated.status(), ReqStatus::OK);
    let isolated_pull = pull(&client, &base, isolated_route, None, 3).await;
    assert_eq!(isolated_pull.status(), ReqStatus::OK);
    let isolated_body: PullResp = isolated_pull.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(isolated_body.items.len(), 1);
    assert_eq!(isolated_body.items[0].id, "NA0277_ISOLATED_ACCEPTED");
    assert_eq!(isolated_body.items[0].data.as_slice(), b"isolated-ok");

    let drain = pull(&client, &base, overloaded_route, None, 99).await;
    assert_eq!(drain.status(), ReqStatus::OK);
    let body: PullResp = drain.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 3);
    for (idx, item) in body.items.iter().enumerate() {
        assert_eq!(item.id, format!("NA0277_ACCEPTED_{idx}"));
        assert_eq!(item.data.as_slice(), format!("accepted-{idx}").as_bytes());
    }

    let empty_after_drain = pull(&client, &base, overloaded_route, None, 1).await;
    assert_eq!(empty_after_drain.status(), ReqStatus::NO_CONTENT);

    let reusable = push(
        &client,
        &base,
        overloaded_route,
        None,
        Some("NA0277_AFTER_DRAIN"),
        b"after-drain".to_vec(),
    )
    .await;
    assert_eq!(reusable.status(), ReqStatus::OK);
    let after_drain = pull(&client, &base, overloaded_route, None, 1).await;
    assert_eq!(after_drain.status(), ReqStatus::OK);
    let after_drain_body: PullResp = after_drain.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(after_drain_body.items.len(), 1);
    assert_eq!(after_drain_body.items[0].id, "NA0277_AFTER_DRAIN");
    assert_eq!(after_drain_body.items[0].data.as_slice(), b"after-drain");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn body_and_auth_rejects_under_pressure_do_not_mutate() {
    let auth_token = "NA0277_AUTH_TOKEN_SENTINEL";
    let wrong_auth = "NA0277_WRONG_AUTH_SENTINEL";
    let (base, handle) = spawn_server_with_auth(Limits::new(4, 1).unwrap(), Some(auth_token)).await;
    let client = reqwest::Client::new();

    let oversize = push(
        &client,
        &base,
        "NA0277_OVERSIZE_ROUTE",
        Some(auth_token),
        Some("NA0277_OVERSIZE_ID"),
        b"too-large".to_vec(),
    )
    .await;
    assert_eq!(oversize.status(), ReqStatus::PAYLOAD_TOO_LARGE);
    assert_eq!(
        oversize.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_TOO_LARGE"
    );
    let empty_after_oversize =
        pull(&client, &base, "NA0277_OVERSIZE_ROUTE", Some(auth_token), 1).await;
    assert_eq!(empty_after_oversize.status(), ReqStatus::NO_CONTENT);

    let pressure_route = "NA0277_PRESSURE_ROUTE";
    let accepted = push(
        &client,
        &base,
        pressure_route,
        Some(auth_token),
        Some("NA0277_PRESSURE_ACCEPTED"),
        b"fits".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let missing_auth = client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, pressure_route)
        .body(b"miss".to_vec())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(missing_auth.status(), ReqStatus::UNAUTHORIZED);
    assert_eq!(
        missing_auth.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_UNAUTHORIZED"
    );

    let wrong_auth_reject = push(
        &client,
        &base,
        pressure_route,
        Some(wrong_auth),
        Some("NA0277_WRONG_AUTH_ID"),
        b"bad!".to_vec(),
    )
    .await;
    assert_eq!(wrong_auth_reject.status(), ReqStatus::UNAUTHORIZED);
    assert_eq!(
        wrong_auth_reject
            .text()
            .await
            .unwrap_or_else(|e| panic!("{e}")),
        "ERR_UNAUTHORIZED"
    );

    let overloaded = push(
        &client,
        &base,
        pressure_route,
        Some(auth_token),
        Some("NA0277_OVERLOAD_ID"),
        b"full".to_vec(),
    )
    .await;
    assert_eq!(overloaded.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        overloaded.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_OVERLOADED"
    );

    let delivered = pull(&client, &base, pressure_route, Some(auth_token), 10).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0277_PRESSURE_ACCEPTED");
    assert_eq!(body.items[0].data.as_slice(), b"fits");

    let empty_after_delivery = pull(&client, &base, pressure_route, Some(auth_token), 1).await;
    assert_eq!(empty_after_delivery.status(), ReqStatus::NO_CONTENT);

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn pressure_logs_redact_route_auth_payload_and_keep_msg_id_boundary() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let writer = SharedWriter(buf.clone());
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = set_default(subscriber);

    let route_token = "NA0277_ROUTE_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let auth_token = "NA0277_AUTH_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let wrong_auth = "NA0277_WRONG_AUTH_SENTINEL_MUST_NOT_LEAK";
    let msg_id = "NA0277_MSG_ID_NONSECRET_METADATA";
    let payload = b"NA0277_PAYLOAD_SENTINEL_MUST_NOT_LEAK".to_vec();
    let overload_payload = b"NA0277_OVERLOAD_PAYLOAD_MUST_NOT_LEAK".to_vec();
    let reject_payload = b"NA0277_REJECT_PAYLOAD_MUST_NOT_LEAK".to_vec();

    let (base, handle) =
        spawn_server_with_auth(Limits::new(128, 1).unwrap(), Some(auth_token)).await;
    let client = reqwest::Client::new();

    let accepted = push(
        &client,
        &base,
        route_token,
        Some(auth_token),
        Some(msg_id),
        payload,
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let overloaded = push(
        &client,
        &base,
        route_token,
        Some(auth_token),
        Some("NA0277_OVERLOAD_MSG_ID"),
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
        Some("NA0277_REJECT_MSG_ID"),
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
    assert_eq!(body.items[0].id, msg_id);
    assert_eq!(
        body.items[0].data.as_slice(),
        b"NA0277_PAYLOAD_SENTINEL_MUST_NOT_LEAK"
    );

    tokio::task::yield_now().await;
    handle.abort();

    let guard = buf.lock().unwrap_or_else(|e| panic!("{e}"));
    let logs = String::from_utf8_lossy(&guard);
    assert!(logs.contains("push channel_id="));
    assert!(logs.contains("pull channel_id="));
    assert!(logs.contains("event=overloaded"));
    assert!(logs.contains(msg_id));

    for forbidden in [
        route_token,
        auth_token,
        wrong_auth,
        "Authorization",
        "Bearer",
        "NA0277_PAYLOAD_SENTINEL_MUST_NOT_LEAK",
        "NA0277_OVERLOAD_PAYLOAD_MUST_NOT_LEAK",
        "NA0277_REJECT_PAYLOAD_MUST_NOT_LEAK",
    ] {
        assert!(!logs.contains(forbidden), "logs leaked {forbidden}");
    }
}

#[test]
fn rate_limit_and_global_route_cap_are_explicit_future_gaps_not_claimed() {
    let readme = include_str!("../README.md");
    let inbox_contract =
        include_str!("../docs/server/DOC-SRV-003_Relay_Inbox_Contract_v1.0.0_DRAFT.md");

    for doc in [readme, inbox_contract] {
        assert!(doc.contains("No in-app rate limiting is implemented"));
        assert!(doc.contains("No global route-count cap is implemented"));
    }
}
