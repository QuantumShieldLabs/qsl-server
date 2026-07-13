// NA-0642 at-rest posture: raw route tokens are never persisted — the store
// keys routes by SHA-256(route_token). Payload bytes ARE stored verbatim
// (they are opaque E2EE ciphertext by contract), which doubles as the
// non-vacuity control proving we are reading the right file.

use qsl_server::{app, AppState, Limits, ResourceControls, StoreConfig};
use reqwest::StatusCode as ReqStatus;
use tokio::net::TcpListener;

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[tokio::test(flavor = "current_thread")]
async fn raw_route_token_never_touches_the_store_file() {
    let dir = std::env::temp_dir().join(format!("na0642-privacy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let db_path = dir.join("relay.db");

    let route_token = "NA0642_PRIVACY_ROUTE_TOKEN_MUST_NOT_TOUCH_DISK";
    let payload = b"NA0642_PRIVACY_OPAQUE_PAYLOAD_MARKER".to_vec();

    let store = StoreConfig {
        path: db_path.to_string_lossy().into_owned(),
        ..StoreConfig::default()
    };
    let state = AppState::new_with_auth_controls_and_store(
        Limits::new(1024, 8).unwrap(),
        ResourceControls::new(4, 8, 8).unwrap(),
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
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let accepted = client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .body(payload.clone())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(accepted.status(), ReqStatus::OK);
    handle.abort();

    // Read every store artifact (main db + WAL + shm if present).
    let mut on_disk = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let path = format!("{}{}", db_path.to_string_lossy(), suffix);
        if let Ok(bytes) = std::fs::read(&path) {
            on_disk.extend_from_slice(&bytes);
        }
    }
    assert!(!on_disk.is_empty(), "store files missing or empty");

    // Non-vacuity: the opaque payload IS on disk verbatim (right file, real
    // write path)...
    assert!(
        contains_subslice(&on_disk, &payload),
        "payload bytes not found — wrong file or broken write path"
    );
    // ...while the raw route token never is.
    assert!(
        !contains_subslice(&on_disk, route_token.as_bytes()),
        "raw route token leaked into the store file"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
