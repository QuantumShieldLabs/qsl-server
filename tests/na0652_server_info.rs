// NA-0652 capability document contract (D588; DOC-SRV-006):
// - GET /v1/server-info on an OPEN relay (no RELAY_TOKEN) returns the full
//   document to any request, auth.mode == "open".
// - On a BEARER relay an unauthorized request (missing OR wrong token —
//   identical both ways, no oracle) gets HTTP 401 with EXACTLY the fixed
//   two-key probe {"server":"qsl-server","auth":{"mode":"bearer"}}; a valid
//   token gets the full document, auth.mode == "bearer".
// - Document values come from LIVE config (injected limits / retention TTL /
//   RELAY_NAME / RELAY_ATTACHMENTS_SERVICE_URL / RELAY_MIN_CLIENT_VERSION),
//   never constants; name is ""-safe, the other two are null-safe.
// - The top-level field set is exact: additions must consciously update the
//   guard here (additive-only evolution).

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use qsl_server::{app, AppState, Limits, ResourceControls, ServerInfoCfg, StoreConfig};
use reqwest::StatusCode as ReqStatus;
use serde_json::Value;
use tokio::net::TcpListener;

async fn spawn_server(
    limits: Limits,
    relay_token: Option<String>,
    store: StoreConfig,
    info: ServerInfoCfg,
) -> (String, tokio::task::JoinHandle<()>) {
    let state = AppState::new_with_auth_controls_store_and_info(
        limits,
        ResourceControls::new(8, 16, 16).unwrap(),
        relay_token,
        store,
        info,
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

async fn spawn_default(relay_token: Option<String>) -> (String, tokio::task::JoinHandle<()>) {
    spawn_server(
        Limits::new(1024 * 1024, 16).unwrap(),
        relay_token,
        StoreConfig::default(),
        ServerInfoCfg::default(),
    )
    .await
}

fn expected_probe() -> Value {
    serde_json::json!({
        "server": "qsl-server",
        "auth": { "mode": "bearer" }
    })
}

#[tokio::test]
async fn open_relay_unauthenticated_gets_full_document() {
    let (base, _h) = spawn_default(None).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base}/v1/server-info"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(resp.status(), ReqStatus::OK);
    let doc: Value = resp.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(doc["server"], "qsl-server");
    assert_eq!(doc["auth"]["mode"], "open");
    assert_eq!(doc["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn bearer_missing_token_gets_exact_probe_401() {
    let (base, _h) = spawn_default(Some("topsecret".to_string())).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base}/v1/server-info"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(resp.status(), ReqStatus::UNAUTHORIZED);
    let body: Value = resp.json().await.unwrap_or_else(|e| panic!("{e}"));
    // Exact-value equality doubles as the exact-field-set guard: any extra or
    // missing key at either nesting level fails this comparison.
    assert_eq!(body, expected_probe());
    let top = body.as_object().unwrap_or_else(|| panic!("not an object"));
    assert_eq!(top.len(), 2);
    let auth = body["auth"]
        .as_object()
        .unwrap_or_else(|| panic!("auth not an object"));
    assert_eq!(auth.len(), 1);
}

#[tokio::test]
async fn bearer_wrong_token_gets_byte_identical_probe() {
    let (base, _h) = spawn_default(Some("topsecret".to_string())).await;
    let client = reqwest::Client::new();
    let missing = client
        .get(format!("{base}/v1/server-info"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let missing_status = missing.status();
    let missing_bytes = missing.bytes().await.unwrap_or_else(|e| panic!("{e}"));
    let wrong = client
        .get(format!("{base}/v1/server-info"))
        .header("Authorization", "Bearer wrong")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(missing_status, ReqStatus::UNAUTHORIZED);
    assert_eq!(wrong.status(), ReqStatus::UNAUTHORIZED);
    let wrong_bytes = wrong.bytes().await.unwrap_or_else(|e| panic!("{e}"));
    // No oracle: a wrong token is indistinguishable from a missing one.
    assert_eq!(missing_bytes, wrong_bytes);
    let parsed: Value = serde_json::from_slice(&missing_bytes).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(parsed, expected_probe());
}

#[tokio::test]
async fn bearer_valid_token_gets_full_document() {
    let (base, _h) = spawn_default(Some("topsecret".to_string())).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base}/v1/server-info"))
        .header("Authorization", "Bearer topsecret")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(resp.status(), ReqStatus::OK);
    let doc: Value = resp.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(doc["server"], "qsl-server");
    assert_eq!(doc["auth"]["mode"], "bearer");
}

#[tokio::test]
async fn document_values_track_injected_config() {
    let (base, _h) = spawn_server(
        Limits::new(4096, 9).unwrap(),
        None,
        StoreConfig {
            retention_ttl_secs: 3600,
            ..StoreConfig::default()
        },
        ServerInfoCfg {
            name: Some("Ops Relay".to_string()),
            attachments_service_url: Some("https://attach.example".to_string()),
            min_client_version: Some("0.9.0".to_string()),
        },
    )
    .await;
    let client = reqwest::Client::new();
    let doc: Value = client
        .get(format!("{base}/v1/server-info"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .json()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(doc["limits"]["max_body_bytes"], 4096);
    assert_eq!(doc["limits"]["max_queue_depth"], 9);
    assert_eq!(doc["retention"]["ttl_secs"], 3600);
    assert_eq!(doc["name"], "Ops Relay");
    assert_eq!(doc["attachments"]["service_url"], "https://attach.example");
    assert_eq!(doc["min_client_version"], "0.9.0");
    assert_eq!(doc["version"], env!("CARGO_PKG_VERSION"));
    // NA-0678: `invite_v1` appended. This guard is EXACT by design -- it is
    // meant to fail on any change to the advertised API set, and it moves in
    // the same commit as the change (D614 §2d).
    assert_eq!(
        doc["api"],
        serde_json::json!(["push_v1", "pull_v1", "pull_ack_lease_v1", "invite_v1"])
    );
    assert_eq!(doc["directory"]["mode"], "none");
    assert_eq!(doc["kt"]["mode"], "none");
}

#[tokio::test]
async fn document_optional_fields_are_empty_and_null_safe_when_unset() {
    let (base, _h) = spawn_default(None).await;
    let client = reqwest::Client::new();
    let doc: Value = client
        .get(format!("{base}/v1/server-info"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .json()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(doc["name"], "");
    assert!(doc["attachments"]["service_url"].is_null());
    assert!(doc["min_client_version"].is_null());
}

#[tokio::test]
async fn full_document_top_level_field_set_is_exact() {
    let (base, _h) = spawn_default(None).await;
    let client = reqwest::Client::new();
    let doc: Value = client
        .get(format!("{base}/v1/server-info"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .json()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let mut keys: Vec<&str> = doc
        .as_object()
        .unwrap_or_else(|| panic!("not an object"))
        .keys()
        .map(|k| k.as_str())
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "api",
            "attachments",
            "auth",
            "directory",
            // NA-0678: the invite capability block. Additive -- every pre-existing
            // key is still present and unrenamed.
            "invite",
            "kt",
            "limits",
            "min_client_version",
            "name",
            "retention",
            "server",
            "version",
        ]
    );
}

// End-to-end env plumbing through the real binary: the three RELAY_-form vars
// (and RELAY_TOKEN) enter via the process environment exactly as an operator
// sets them, proving the main.rs -> AppState -> document path with nothing
// injected in-process.
struct Relay {
    child: Child,
    base: String,
}

impl Relay {
    fn spawn(extra_envs: &[(&str, &str)]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_qsl-server"));
        command
            .env_clear()
            .env("RUST_LOG", "info")
            .env("BIND_ADDR", "127.0.0.1")
            .env("PORT", "0")
            .env("STORE_PATH", ":memory:")
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (name, value) in extra_envs {
            command.env(name, value);
        }
        let mut child = command.spawn().unwrap_or_else(|e| panic!("{e}"));
        let stdout = child.stdout.take().unwrap_or_else(|| panic!("no stdout"));
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            let mut sent = false;
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if !sent {
                    if let Some(idx) = line.find("listening on ") {
                        let addr = line[idx + "listening on ".len()..].trim().to_string();
                        let _ = tx.send(addr);
                        sent = true;
                    }
                }
            }
        });
        let addr = rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|e| panic!("relay did not report listen address: {e}"));
        Self {
            child,
            base: format!("http://{addr}"),
        }
    }

    fn stop(mut self) {
        self.child.kill().unwrap_or_else(|e| panic!("{e}"));
        self.child.wait().unwrap_or_else(|e| panic!("{e}"));
    }
}

#[tokio::test]
async fn relay_env_vars_flow_to_document_end_to_end() {
    let relay = Relay::spawn(&[
        ("RELAY_TOKEN", "envsecret"),
        ("RELAY_NAME", "Env Relay"),
        (
            "RELAY_ATTACHMENTS_SERVICE_URL",
            "https://attach.env.example",
        ),
        ("RELAY_MIN_CLIENT_VERSION", "1.2.3"),
        ("RETENTION_TTL_SECS", "7200"),
    ]);
    let client = reqwest::Client::new();

    let probe = client
        .get(format!("{}/v1/server-info", relay.base))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(probe.status(), ReqStatus::UNAUTHORIZED);
    let probe_body: Value = probe.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(probe_body, expected_probe());

    let doc: Value = client
        .get(format!("{}/v1/server-info", relay.base))
        .header("Authorization", "Bearer envsecret")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .json()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(doc["auth"]["mode"], "bearer");
    assert_eq!(doc["name"], "Env Relay");
    assert_eq!(
        doc["attachments"]["service_url"],
        "https://attach.env.example"
    );
    assert_eq!(doc["min_client_version"], "1.2.3");
    assert_eq!(doc["retention"]["ttl_secs"], 7200);
    relay.stop();

    let bare = Relay::spawn(&[]);
    let doc: Value = client
        .get(format!("{}/v1/server-info", bare.base))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .json()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(doc["auth"]["mode"], "open");
    assert_eq!(doc["name"], "");
    assert!(doc["attachments"]["service_url"].is_null());
    assert!(doc["min_client_version"].is_null());
    bare.stop();
}
