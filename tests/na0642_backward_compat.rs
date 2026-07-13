// NA-0642 backward-compat guard: the legacy pull contract (no ack parameter)
// must stay byte-identical for the CURRENT non-acking qsc client — same
// delete-on-deliver semantics, same JSON shape with exactly the same fields,
// same 204 behavior. The NA-0640 full-stack e2e relies on this at pin-bump
// time.

use qsl_server::{app, AppState, Limits};
use reqwest::StatusCode as ReqStatus;
use serde_json::Value;
use tokio::net::TcpListener;

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const MSG_ID_HEADER: &str = "X-Msg-Id";

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let state = AppState::new_with_auth(Limits::new(1024 * 1024, 8).unwrap(), None);
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

#[tokio::test(flavor = "current_thread")]
async fn legacy_pull_shape_and_semantics_are_unchanged() {
    let (base, handle) = spawn_server().await;
    let client = reqwest::Client::new();
    let route = "NA0642_COMPAT_ROUTE";

    let push = client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, route)
        .header(MSG_ID_HEADER, "NA0642_COMPAT_MSG")
        .body(b"compat-payload".to_vec())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(push.status(), ReqStatus::OK);
    // Push response shape: exactly {"id": ...}.
    let push_body: Value = push.json().await.unwrap_or_else(|e| panic!("{e}"));
    let push_obj = push_body
        .as_object()
        .unwrap_or_else(|| panic!("not object"));
    assert_eq!(push_obj.len(), 1);
    assert_eq!(
        push_obj.get("id").and_then(Value::as_str),
        Some("NA0642_COMPAT_MSG")
    );

    // Legacy pull (no ack parameter): today's exact contract.
    let pull = client
        .get(format!("{base}/v1/pull?max=2"))
        .header(ROUTE_TOKEN_HEADER, route)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(pull.status(), ReqStatus::OK);
    let pull_body: Value = pull.json().await.unwrap_or_else(|e| panic!("{e}"));
    let pull_obj = pull_body
        .as_object()
        .unwrap_or_else(|| panic!("not object"));
    // Top level: exactly {"items": [...]}.
    assert_eq!(pull_obj.len(), 1);
    let items = pull_obj
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("items missing"));
    assert_eq!(items.len(), 1);
    // Item: exactly {"id": ..., "data": [...]} — no new fields leak into the
    // legacy response.
    let item = items[0].as_object().unwrap_or_else(|| panic!("not object"));
    assert_eq!(item.len(), 2);
    assert_eq!(
        item.get("id").and_then(Value::as_str),
        Some("NA0642_COMPAT_MSG")
    );
    let data: Vec<u8> = item
        .get("data")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("data missing"))
        .iter()
        .map(|v| v.as_u64().unwrap_or_else(|| panic!("not byte")) as u8)
        .collect();
    assert_eq!(data.as_slice(), b"compat-payload");

    // Delete-on-deliver: a non-acking client drains without any ack.
    let drained = client
        .get(format!("{base}/v1/pull?max=1"))
        .header(ROUTE_TOKEN_HEADER, route)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(drained.status(), ReqStatus::NO_CONTENT);

    handle.abort();
}
