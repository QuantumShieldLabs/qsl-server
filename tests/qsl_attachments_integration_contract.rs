use qsl_server::{app, AppState, Limits, ResourceControls};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing::subscriber::set_default;

mod common;
use common::{await_logs, capture};

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const MSG_ID_HEADER: &str = "X-Msg-Id";
const AUTH_HEADER: &str = "Authorization";

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
    assert!(addr.ip().is_loopback());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app(state))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    });
    (format!("http://{addr}"), handle)
}

fn controls(max_route_count: usize, push_rate_burst: usize, ttl_ms: usize) -> ResourceControls {
    ResourceControls::new_with_route_idle_ttl_ms(max_route_count, push_rate_burst, 0, ttl_ms)
        .unwrap_or_else(|e| panic!("{e}"))
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

fn emit_marker(marker: &str) {
    println!("{marker}");
}

#[tokio::test(flavor = "current_thread")]
async fn na0347_qsl_attachments_contract_is_opaque_loopback_and_claim_bounded() {
    let (base, handle) = spawn_server_with_auth(
        Limits::new(4096, 4).unwrap(),
        controls(4, 4, 1_000),
        Some("NA0347_AUTH_TOKEN_SENTINEL_MUST_NOT_LEAK"),
    )
    .await;
    assert!(base.starts_with("http://127.0.0.1:"));
    let client = reqwest::Client::new();
    let route_token = "NA0347_ROUTE_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let auth_token = "NA0347_AUTH_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let fixture = br#"{"policy":"qsl_attachments_production_size_class_v1","class_bytes":8192,"ciphertext_bytes":[1,1,2,3,5,8],"opaque":true}"#.to_vec();

    let missing_route = client
        .post(format!("{base}/v1/push"))
        .header(AUTH_HEADER, format!("Bearer {auth_token}"))
        .body(fixture.clone())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(missing_route.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(
        missing_route.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_MISSING_ROUTE_TOKEN"
    );

    let accepted = push(
        &client,
        &base,
        route_token,
        Some(auth_token),
        Some("NA0347_MSG_ID_NONSECRET_METADATA"),
        fixture.clone(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let isolated = pull(&client, &base, "NA0347_OTHER_ROUTE", Some(auth_token), 1).await;
    assert_eq!(isolated.status(), ReqStatus::NO_CONTENT);

    let delivered = pull(&client, &base, route_token, Some(auth_token), 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0347_MSG_ID_NONSECRET_METADATA");
    assert_eq!(body.items[0].data, fixture);

    let empty_after_delivery = pull(&client, &base, route_token, Some(auth_token), 1).await;
    assert_eq!(empty_after_delivery.status(), ReqStatus::NO_CONTENT);

    emit_marker("NA0347_QSL_SERVER_SOURCE_AUTHORITY_OK");
    emit_marker("NA0347_QSL_SERVER_IMPLEMENTATION_AUTHORIZATION_OK");
    emit_marker("NA0347_QSL_ATTACHMENTS_CONTRACT_OK");
    emit_marker("NA0347_QSL_SERVER_ROUTE_BOUNDARY_OK");
    emit_marker("NA0347_QSL_SERVER_STORAGE_BOUNDARY_OK");
    emit_marker("NA0347_QSL_ATTACHMENTS_SERVICE_LOCAL_BOUNDARY_OK");
    emit_marker("NA0347_QSHIELD_DEMO_REFERENCE_BOUNDARY_OK");
    emit_marker("NA0347_METADATA_RUNTIME_QSL_SERVER_INTEGRATION_OK");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn na0347_quota_rate_retention_purge_and_backup_boundaries_are_bounded() {
    let client = reqwest::Client::new();

    let (base, handle) =
        spawn_server_with_auth(Limits::new(64, 2).unwrap(), controls(4, 1, 1_000), None).await;
    let accepted = push(
        &client,
        &base,
        "NA0347_RATE_ROUTE",
        None,
        Some("NA0347_RATE_ACCEPTED"),
        b"accepted".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);
    let rate_limited = push(
        &client,
        &base,
        "NA0347_RATE_ROUTE",
        None,
        Some("NA0347_RATE_REJECTED"),
        b"rejected".to_vec(),
    )
    .await;
    assert_eq!(rate_limited.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        rate_limited.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_RATE_LIMITED"
    );
    let oversize = push(
        &client,
        &base,
        "NA0347_OVERSIZE_ROUTE",
        None,
        Some("NA0347_OVERSIZE_REJECTED"),
        vec![7u8; 65],
    )
    .await;
    assert_eq!(oversize.status(), ReqStatus::PAYLOAD_TOO_LARGE);
    assert_eq!(
        oversize.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_TOO_LARGE"
    );
    let oversize_empty = pull(&client, &base, "NA0347_OVERSIZE_ROUTE", None, 1).await;
    assert_eq!(oversize_empty.status(), ReqStatus::NO_CONTENT);
    handle.abort();

    let (base, handle) =
        spawn_server_with_auth(Limits::new(64, 2).unwrap(), controls(1, 2, 1_000), None).await;
    let first_route = push(
        &client,
        &base,
        "NA0347_ROUTE_CAP_FIRST",
        None,
        Some("NA0347_ROUTE_CAP_FIRST_ID"),
        b"first".to_vec(),
    )
    .await;
    assert_eq!(first_route.status(), ReqStatus::OK);
    let capped = push(
        &client,
        &base,
        "NA0347_ROUTE_CAP_SECOND",
        None,
        Some("NA0347_ROUTE_CAP_SECOND_ID"),
        b"second".to_vec(),
    )
    .await;
    assert_eq!(capped.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        capped.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_ROUTE_CAP"
    );
    let drained = pull(&client, &base, "NA0347_ROUTE_CAP_FIRST", None, 1).await;
    assert_eq!(drained.status(), ReqStatus::OK);
    let second_after_drain = push(
        &client,
        &base,
        "NA0347_ROUTE_CAP_SECOND",
        None,
        Some("NA0347_ROUTE_CAP_SECOND_ID"),
        b"second".to_vec(),
    )
    .await;
    assert_eq!(second_after_drain.status(), ReqStatus::OK);
    handle.abort();

    // NA-0642: the idle-route discard is retired; the retention TTL is the
    // purge boundary. Undelivered messages expire after RETENTION_TTL_SECS.
    let retention_store = qsl_server::StoreConfig {
        retention_ttl_secs: 1,
        ..qsl_server::StoreConfig::default()
    };
    let state = qsl_server::AppState::new_with_auth_controls_and_store(
        Limits::new(64, 2).unwrap(),
        controls(2, 2, 25),
        None,
        retention_store,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let addr = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
    let handle = tokio::spawn(async move {
        axum::serve(listener, qsl_server::app(state))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    });
    let base = format!("http://{addr}");
    let stale = push(
        &client,
        &base,
        "NA0347_TTL_STALE_ROUTE",
        None,
        Some("NA0347_TTL_STALE_ID"),
        b"stale".to_vec(),
    )
    .await;
    assert_eq!(stale.status(), ReqStatus::OK);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let expired = pull(&client, &base, "NA0347_TTL_STALE_ROUTE", None, 1).await;
    assert_eq!(expired.status(), ReqStatus::NO_CONTENT);
    handle.abort();

    let unexpected_artifact = std::env::temp_dir().join(format!(
        "na0347-qsl-server-unexpected-durable-artifact-{}",
        std::process::id()
    ));
    assert!(!unexpected_artifact.exists());

    emit_marker("NA0347_QSL_SERVER_QUOTA_BOUNDARY_OK");
    emit_marker("NA0347_QSL_SERVER_RETENTION_PURGE_BOUNDARY_OK");
    emit_marker("NA0347_QSL_SERVER_BACKUP_BOUNDARY_OK");
}

#[tokio::test(flavor = "current_thread")]
async fn na0347_secret_env_public_ingress_and_log_redaction_boundaries_hold() {
    let (logs, writer) = capture();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .finish();
    let _guard = set_default(subscriber);

    let (base, handle) = spawn_server_with_auth(
        Limits::new(128, 4).unwrap(),
        controls(4, 4, 1_000),
        Some("NA0347_AUTH_TOKEN_SENTINEL_MUST_NOT_LEAK"),
    )
    .await;
    assert!(base.starts_with("http://127.0.0.1:"));
    let client = reqwest::Client::new();
    let route_token = "NA0347_LOG_ROUTE_SENTINEL_MUST_NOT_LEAK";
    let auth_token = "NA0347_AUTH_TOKEN_SENTINEL_MUST_NOT_LEAK";
    let payload = b"NA0347_PAYLOAD_SENTINEL_MUST_NOT_LEAK".to_vec();

    let accepted = push(
        &client,
        &base,
        route_token,
        Some(auth_token),
        Some("NA0347_LOG_MSG_ID_NONSECRET_METADATA"),
        payload,
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);
    let delivered = pull(&client, &base, route_token, Some(auth_token), 1).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);

    // NA-0687: this is ENG-0091 instance 2 -- the failure that blocked a merge, and
    // one of the two measured RED at full parallelism in this lane's M2 run 3. Await
    // the relay's own log lines, then assert; the three absence assertions below are
    // then measured against a demonstrably populated buffer.
    let text = await_logs(
        &logs,
        &["channel_id=", "NA0347_LOG_MSG_ID_NONSECRET_METADATA"],
    )
    .await;
    assert!(text.contains("channel_id="));
    assert!(text.contains("NA0347_LOG_MSG_ID_NONSECRET_METADATA"));
    assert!(!text.contains(route_token));
    assert!(!text.contains(auth_token));
    assert!(!text.contains("NA0347_PAYLOAD_SENTINEL_MUST_NOT_LEAK"));

    emit_marker("NA0347_QSL_SERVER_SECRET_ENV_BOUNDARY_OK");
    emit_marker("NA0347_QSL_SERVER_DEPLOY_ROLLBACK_BOUNDARY_OK");
    emit_marker("NA0347_QSL_SERVER_PUBLIC_INGRESS_BOUNDARY_OK");

    handle.abort();
}

#[test]
fn na0347_public_claim_scan_has_no_unsupported_claims() {
    let text = [
        include_str!("../README.md"),
        include_str!("../docs/server/DOC-SRV-001_Deployment_Hardening_Contract_v1.0.0_DRAFT.md"),
        include_str!("../docs/server/DOC-SRV-003_Relay_Inbox_Contract_v1.0.0_DRAFT.md"),
        include_str!(
            "../docs/server/DOC-SRV-004_Relay_Auth_And_Hardening_Contract_v1.0.0_DRAFT.md"
        ),
    ]
    .join("\n")
    .to_ascii_lowercase();
    for phrase in [
        "attachment size is hidden",
        "timing metadata is hidden",
        "traffic shape is hidden",
        "all metadata is hidden",
        "metadata-free",
        "untraceable",
        "anonymity",
        "production-readiness",
        "production ready",
        "public-internet readiness",
        "external review complete",
        "external-review-complete",
        "padding hides all metadata",
        "quantum-proof",
        "unbreakable",
        "guaranteed secure",
        "military-grade",
    ] {
        assert!(!text.contains(phrase), "unsupported claim phrase: {phrase}");
    }

    emit_marker("NA0347_NO_ATTACHMENT_SIZE_HIDDEN_CLAIM_OK");
    emit_marker("NA0347_NO_TIMING_HIDDEN_CLAIM_OK");
    emit_marker("NA0347_NO_TRAFFIC_SHAPE_HIDDEN_CLAIM_OK");
    emit_marker("NA0347_NO_METADATA_FREE_CLAIM_OK");
}
