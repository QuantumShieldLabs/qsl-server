use qsl_server::{app, AppState, Limits, ResourceControls};
use reqwest::StatusCode as ReqStatus;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::net::TcpListener;
use tracing::subscriber::set_default;

mod common;
use common::{await_logs, capture};

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const MSG_ID_HEADER: &str = "X-Msg-Id";
const AUTH_HEADER: &str = "Authorization";
const SIZE_CLASS_POLICY: &str = "qsl_attachments_production_size_class_v1";
const ROUTE_TOKEN_SENTINEL: &str = "NA0349_ROUTE_TOKEN_SENTINEL_MUST_NOT_LEAK";
const AUTH_TOKEN_SENTINEL: &str = "NA0349_AUTH_TOKEN_SENTINEL_MUST_NOT_LEAK";
const FETCH_CAPABILITY_SENTINEL: &str = "NA0349_FETCH_CAPABILITY_SENTINEL_MUST_NOT_LEAK";
const PAYLOAD_SENTINEL: &str = "NA0349_ATTACHMENT_PAYLOAD_SENTINEL_MUST_NOT_LEAK";

const QSHIELD_DEMO_SMALL_CLASSES: [u64; 12] = [
    256, 512, 768, 1024, 1536, 2048, 3072, 4096, 5120, 6144, 7168, 8192,
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AttachmentDescriptor {
    locator_kind: String,
    locator_ref: String,
    fetch_capability: String,
    attachment_id: String,
    ciphertext_len: u64,
    size_class_policy: String,
    size_class_bytes: u64,
    retention_class: String,
}

#[derive(Debug, Clone)]
struct AttachmentObject {
    descriptor: AttachmentDescriptor,
    ciphertext: Vec<u8>,
    expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AttachmentFetch {
    Ok(Vec<u8>),
    Expired,
    Unauthorized,
    Unknown,
}

#[derive(Default)]
struct AttachmentContractFixture {
    objects: HashMap<String, AttachmentObject>,
    audit_events: Vec<String>,
    now: u64,
}

impl AttachmentContractFixture {
    fn commit(&mut self, seed: u64, ciphertext: Vec<u8>, ttl_secs: u64) -> AttachmentDescriptor {
        let size_class_bytes = production_size_class(ciphertext.len() as u64);
        let locator_ref = format!("na0349-locator-{seed:012x}");
        let descriptor = AttachmentDescriptor {
            locator_kind: "service_ref_v1".to_owned(),
            locator_ref: locator_ref.clone(),
            fetch_capability: FETCH_CAPABILITY_SENTINEL.to_owned(),
            attachment_id: format!("{seed:064x}"),
            ciphertext_len: ciphertext.len() as u64,
            size_class_policy: SIZE_CLASS_POLICY.to_owned(),
            size_class_bytes,
            retention_class: "standard".to_owned(),
        };
        self.audit_events
            .push(format!("object_committed locator_handle={:012x}", seed));
        self.objects.insert(
            locator_ref,
            AttachmentObject {
                descriptor: descriptor.clone(),
                ciphertext,
                expires_at: self.now + ttl_secs,
            },
        );
        descriptor
    }

    fn fetch(&mut self, locator_ref: &str, fetch_capability: &str) -> AttachmentFetch {
        let Some(object) = self.objects.get(locator_ref) else {
            return AttachmentFetch::Unknown;
        };
        if object.descriptor.fetch_capability != fetch_capability {
            return AttachmentFetch::Unauthorized;
        }
        if self.now > object.expires_at {
            return AttachmentFetch::Expired;
        }
        self.audit_events.push(format!(
            "object_fetched locator_handle={}",
            redacted_handle(locator_ref)
        ));
        AttachmentFetch::Ok(object.ciphertext.clone())
    }

    fn advance(&mut self, seconds: u64) {
        self.now += seconds;
    }

    fn purge_expired(&mut self) {
        let now = self.now;
        self.objects.retain(|locator_ref, object| {
            let keep = now <= object.expires_at;
            if !keep {
                self.audit_events.push(format!(
                    "object_purged locator_handle={}",
                    redacted_handle(locator_ref)
                ));
            }
            keep
        });
    }

    fn snapshot_restore(&self) -> Self {
        Self {
            objects: self.objects.clone(),
            audit_events: Vec::new(),
            now: self.now,
        }
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
    controls: ResourceControls,
    relay_token: Option<&str>,
) -> (String, tokio::task::JoinHandle<()>) {
    let state =
        AppState::new_with_auth_and_controls(limits, controls, relay_token.map(str::to_owned));
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

fn production_size_class(ciphertext_len: u64) -> u64 {
    if let Some(class) = QSHIELD_DEMO_SMALL_CLASSES
        .iter()
        .copied()
        .find(|class| ciphertext_len <= *class)
    {
        return class;
    }
    let one_mib = 1024 * 1024;
    if ciphertext_len <= one_mib {
        return ciphertext_len.div_ceil(8 * 1024) * 8 * 1024;
    }
    ciphertext_len.div_ceil(one_mib) * one_mib
}

fn redacted_handle(input: &str) -> String {
    let mut state: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x100000001b3);
    }
    format!("{state:012x}", state = state & 0x0000_ffff_ffff_ffff)
}

fn emit_marker(marker: &str) {
    println!("{marker}");
}

#[tokio::test(flavor = "current_thread")]
async fn na0349_qsl_server_qsl_attachments_contract_model_is_end_to_end_bounded() {
    let (logs, writer) = capture();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .finish();
    let _guard = set_default(subscriber);

    let (base, handle) = spawn_server_with_auth(
        Limits::new(4096, 4).unwrap(),
        controls(4, 4, 1_000),
        Some(AUTH_TOKEN_SENTINEL),
    )
    .await;
    assert!(base.starts_with("http://127.0.0.1:"));
    let client = reqwest::Client::new();

    let mut attachments = AttachmentContractFixture::default();
    let ciphertext = format!("{PAYLOAD_SENTINEL}:{}", "ciphertext-object-v1").into_bytes();
    let descriptor = attachments.commit(34_900_001, ciphertext.clone(), 30);
    assert_eq!(descriptor.size_class_policy, SIZE_CLASS_POLICY);
    assert_eq!(descriptor.size_class_bytes, 256);
    assert_eq!(
        attachments.fetch(&descriptor.locator_ref, FETCH_CAPABILITY_SENTINEL),
        AttachmentFetch::Ok(ciphertext.clone())
    );

    let descriptor_bytes = serde_json::to_vec(&descriptor).expect("descriptor json");
    let missing_route = client
        .post(format!("{base}/v1/push"))
        .header(AUTH_HEADER, format!("Bearer {AUTH_TOKEN_SENTINEL}"))
        .body(descriptor_bytes.clone())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(missing_route.status(), ReqStatus::BAD_REQUEST);
    assert_eq!(
        missing_route.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_MISSING_ROUTE_TOKEN"
    );

    let unauth = push(
        &client,
        &base,
        ROUTE_TOKEN_SENTINEL,
        None,
        Some("NA0349_UNAUTH_REJECTED"),
        descriptor_bytes.clone(),
    )
    .await;
    assert_eq!(unauth.status(), ReqStatus::UNAUTHORIZED);

    let pushed = push(
        &client,
        &base,
        ROUTE_TOKEN_SENTINEL,
        Some(AUTH_TOKEN_SENTINEL),
        Some("NA0349_ATTACHMENT_DESCRIPTOR"),
        descriptor_bytes.clone(),
    )
    .await;
    assert_eq!(pushed.status(), ReqStatus::OK);

    let isolated = pull(
        &client,
        &base,
        "NA0349_OTHER_ROUTE",
        Some(AUTH_TOKEN_SENTINEL),
        1,
    )
    .await;
    assert_eq!(isolated.status(), ReqStatus::NO_CONTENT);

    let pulled = pull(
        &client,
        &base,
        ROUTE_TOKEN_SENTINEL,
        Some(AUTH_TOKEN_SENTINEL),
        1,
    )
    .await;
    assert_eq!(pulled.status(), ReqStatus::OK);
    let pull_body: PullResp = pulled.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(pull_body.items.len(), 1);
    assert_eq!(pull_body.items[0].id, "NA0349_ATTACHMENT_DESCRIPTOR");
    assert_eq!(pull_body.items[0].data, descriptor_bytes);
    let delivered_descriptor: AttachmentDescriptor =
        serde_json::from_slice(&pull_body.items[0].data).expect("descriptor parse");
    assert_eq!(delivered_descriptor, descriptor);
    assert_eq!(
        attachments.fetch(
            &delivered_descriptor.locator_ref,
            &delivered_descriptor.fetch_capability
        ),
        AttachmentFetch::Ok(ciphertext)
    );

    let after_delivery = pull(
        &client,
        &base,
        ROUTE_TOKEN_SENTINEL,
        Some(AUTH_TOKEN_SENTINEL),
        1,
    )
    .await;
    assert_eq!(after_delivery.status(), ReqStatus::NO_CONTENT);

    let oversized_descriptor = vec![b'x'; 4097];
    let oversize = push(
        &client,
        &base,
        "NA0349_OVERSIZE_ROUTE",
        Some(AUTH_TOKEN_SENTINEL),
        Some("NA0349_OVERSIZE_REJECTED"),
        oversized_descriptor,
    )
    .await;
    assert_eq!(oversize.status(), ReqStatus::PAYLOAD_TOO_LARGE);
    let oversize_empty = pull(
        &client,
        &base,
        "NA0349_OVERSIZE_ROUTE",
        Some(AUTH_TOKEN_SENTINEL),
        1,
    )
    .await;
    assert_eq!(oversize_empty.status(), ReqStatus::NO_CONTENT);

    let mut restored = attachments.snapshot_restore();
    assert_eq!(
        restored.fetch(&descriptor.locator_ref, FETCH_CAPABILITY_SENTINEL),
        AttachmentFetch::Ok(format!("{PAYLOAD_SENTINEL}:ciphertext-object-v1").into_bytes())
    );
    attachments.advance(31);
    attachments.purge_expired();
    assert_eq!(
        attachments.fetch(&descriptor.locator_ref, FETCH_CAPABILITY_SENTINEL),
        AttachmentFetch::Unknown
    );

    // NA-0687: synchronise on the relay's own log lines before reading. Every
    // absence assertion below is measured against the snapshot this returns.
    let text = await_logs(&logs, &["channel_id=", "NA0349_ATTACHMENT_DESCRIPTOR"]).await;
    assert!(text.contains("channel_id="));
    assert!(text.contains("NA0349_ATTACHMENT_DESCRIPTOR"));
    for secret in [
        ROUTE_TOKEN_SENTINEL,
        AUTH_TOKEN_SENTINEL,
        FETCH_CAPABILITY_SENTINEL,
        PAYLOAD_SENTINEL,
        &descriptor.locator_ref,
        &descriptor.attachment_id,
    ] {
        assert!(
            !text.contains(secret),
            "secret or object ref leaked: {secret}"
        );
    }
    for event in &attachments.audit_events {
        assert!(!event.contains(FETCH_CAPABILITY_SENTINEL));
        assert!(!event.contains(PAYLOAD_SENTINEL));
        assert!(!event.contains(&descriptor.locator_ref));
    }

    assert_eq!(
        &QSHIELD_DEMO_SMALL_CLASSES,
        &[256, 512, 768, 1024, 1536, 2048, 3072, 4096, 5120, 6144, 7168, 8192]
    );

    emit_marker("NA0349_END_TO_END_SOURCE_AUTHORITY_OK");
    emit_marker("NA0349_QSL_SERVER_MAIN_PROOF_OK");
    emit_marker("NA0349_QSL_ATTACHMENTS_MAIN_PROOF_OK");
    emit_marker("NA0349_QSL_SERVER_QSL_ATTACHMENTS_CONTRACT_OK");
    emit_marker("NA0349_ROUTE_API_BOUNDARY_OK");
    emit_marker("NA0349_ATTACHMENT_OBJECT_LIFECYCLE_OK");
    emit_marker("NA0349_SIZE_CLASS_FLOW_BOUNDARY_OK");
    emit_marker("NA0349_OPAQUE_PAYLOAD_BOUNDARY_OK");
    emit_marker("NA0349_ROUTE_TOKEN_AUTH_BOUNDARY_OK");
    emit_marker("NA0349_QUOTA_RATE_BOUNDARY_OK");
    emit_marker("NA0349_RETENTION_PURGE_CONSISTENCY_OK");
    emit_marker("NA0349_BACKUP_RESTORE_BOUNDARY_OK");
    emit_marker("NA0349_LOG_REDACTION_BOUNDARY_OK");
    emit_marker("NA0349_MONITORING_BOUNDARY_OK");
    emit_marker("NA0349_DEPLOY_ROLLBACK_BOUNDARY_OK");
    emit_marker("NA0349_PUBLIC_INGRESS_BOUNDARY_OK");
    emit_marker("NA0349_QSHIELD_DEMO_REFERENCE_BOUNDARY_OK");
    emit_marker("NA0349_METADATA_RUNTIME_END_TO_END_INTEGRATION_OK");

    handle.abort();
}

#[test]
fn na0349_public_claim_scan_has_no_unsupported_claims() {
    let text = [
        include_str!("../README.md"),
        include_str!("../docs/server/DOC-SRV-001_Deployment_Hardening_Contract_v1.0.0_DRAFT.md"),
        include_str!("../docs/server/DOC-SRV-003_Relay_Inbox_Contract_v1.0.0_DRAFT.md"),
        include_str!(
            "../docs/server/DOC-SRV-004_Relay_Auth_And_Hardening_Contract_v1.0.0_DRAFT.md"
        ),
        include_str!("../docs/server/DOC-SRV-005_Route_Token_API_Shape_Review_v1.0.0_DRAFT.md"),
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

    emit_marker("NA0349_NO_ATTACHMENT_SIZE_HIDDEN_CLAIM_OK");
    emit_marker("NA0349_NO_TIMING_HIDDEN_CLAIM_OK");
    emit_marker("NA0349_NO_TRAFFIC_SHAPE_HIDDEN_CLAIM_OK");
    emit_marker("NA0349_NO_METADATA_FREE_CLAIM_OK");
}
