// NA-0642 contended-path coverage: the storage layer moved from a process
// mutex over a HashMap to SQLite behind a connection mutex; these tests pin
// the no-loss/no-duplication guarantee under concurrent pushers and pullers
// (previously untested).

use qsl_server::{app, AppState, Limits, ResourceControls, StoreConfig};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use std::collections::HashSet;
use tokio::net::TcpListener;

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const MSG_ID_HEADER: &str = "X-Msg-Id";

#[derive(Deserialize)]
struct PullItem {
    id: String,
}

#[derive(Deserialize)]
struct PullResp {
    items: Vec<PullItem>,
}

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let state = AppState::new_with_auth_controls_and_store(
        Limits::new(1024, 257).unwrap(),
        ResourceControls::new(8, 257, 4096).unwrap(),
        None,
        StoreConfig::default(),
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_pushes_and_legacy_pulls_lose_and_duplicate_nothing() {
    let (base, handle) = spawn_server().await;
    let route = "NA0642_CONTENTION_LEGACY";
    const PUSHERS: usize = 8;
    const PER_PUSHER: usize = 25;

    let mut push_tasks = Vec::new();
    for p in 0..PUSHERS {
        let base = base.clone();
        push_tasks.push(tokio::spawn(async move {
            let client = reqwest::Client::new();
            for i in 0..PER_PUSHER {
                let id = format!("NA0642_C_{p}_{i}");
                let resp = client
                    .post(format!("{base}/v1/push"))
                    .header(ROUTE_TOKEN_HEADER, route)
                    .header(MSG_ID_HEADER, id.as_str())
                    .body(id.clone().into_bytes())
                    .send()
                    .await
                    .unwrap_or_else(|e| panic!("{e}"));
                assert_eq!(resp.status(), ReqStatus::OK, "push {id} failed");
            }
        }));
    }
    for task in push_tasks {
        task.await.unwrap_or_else(|e| panic!("{e}"));
    }

    // Concurrent legacy pullers drain the route; every message must be
    // delivered exactly once across all pullers.
    const PULLERS: usize = 4;
    let mut pull_tasks = Vec::new();
    for _ in 0..PULLERS {
        let base = base.clone();
        pull_tasks.push(tokio::spawn(async move {
            let client = reqwest::Client::new();
            let mut got: Vec<String> = Vec::new();
            let mut consecutive_empty = 0;
            while consecutive_empty < 3 {
                let resp = client
                    .get(format!("{base}/v1/pull?max=5"))
                    .header(ROUTE_TOKEN_HEADER, route)
                    .send()
                    .await
                    .unwrap_or_else(|e| panic!("{e}"));
                match resp.status() {
                    ReqStatus::OK => {
                        consecutive_empty = 0;
                        let body: PullResp = resp.json().await.unwrap_or_else(|e| panic!("{e}"));
                        got.extend(body.items.into_iter().map(|i| i.id));
                    }
                    ReqStatus::NO_CONTENT => consecutive_empty += 1,
                    other => panic!("unexpected pull status {other}"),
                }
            }
            got
        }));
    }
    let mut all: Vec<String> = Vec::new();
    for task in pull_tasks {
        all.extend(task.await.unwrap_or_else(|e| panic!("{e}")));
    }

    let expected: HashSet<String> = (0..PUSHERS)
        .flat_map(|p| (0..PER_PUSHER).map(move |i| format!("NA0642_C_{p}_{i}")))
        .collect();
    let delivered: HashSet<String> = all.iter().cloned().collect();
    assert_eq!(all.len(), expected.len(), "duplicate delivery detected");
    assert_eq!(delivered, expected, "lost or foreign messages detected");

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_lease_pulls_never_double_deliver_within_the_lease() {
    let (base, handle) = spawn_server().await;
    let route = "NA0642_CONTENTION_LEASE";
    const TOTAL: usize = 50;

    let client = reqwest::Client::new();
    for i in 0..TOTAL {
        let id = format!("NA0642_L_{i}");
        let resp = client
            .post(format!("{base}/v1/push"))
            .header(ROUTE_TOKEN_HEADER, route)
            .header(MSG_ID_HEADER, id.as_str())
            .body(id.clone().into_bytes())
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(resp.status(), ReqStatus::OK);
    }

    // Four concurrent lease-pullers race over the same route; within the
    // lease window no message may be handed to two pullers.
    const PULLERS: usize = 4;
    let mut tasks = Vec::new();
    for _ in 0..PULLERS {
        let base = base.clone();
        tasks.push(tokio::spawn(async move {
            let client = reqwest::Client::new();
            let mut got: Vec<String> = Vec::new();
            loop {
                let resp = client
                    .get(format!("{base}/v1/pull?max=5&ack=lease"))
                    .header(ROUTE_TOKEN_HEADER, route)
                    .send()
                    .await
                    .unwrap_or_else(|e| panic!("{e}"));
                match resp.status() {
                    ReqStatus::OK => {
                        let body: PullResp = resp.json().await.unwrap_or_else(|e| panic!("{e}"));
                        got.extend(body.items.into_iter().map(|i| i.id));
                    }
                    ReqStatus::NO_CONTENT => break,
                    other => panic!("unexpected pull status {other}"),
                }
            }
            got
        }));
    }
    let mut all: Vec<String> = Vec::new();
    for task in tasks {
        all.extend(task.await.unwrap_or_else(|e| panic!("{e}")));
    }
    let unique: HashSet<String> = all.iter().cloned().collect();
    assert_eq!(all.len(), unique.len(), "a message was leased twice");
    assert_eq!(unique.len(), TOTAL, "some messages were never leased");

    // Ack everything; the route must drain completely.
    let ids: Vec<String> = all;
    let resp = client
        .post(format!("{base}/v1/pull/ack"))
        .header(ROUTE_TOKEN_HEADER, route)
        .json(&serde_json::json!({ "ids": ids }))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(resp.status(), ReqStatus::OK);
    let acked: serde_json::Value = resp.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        acked.get("acked").and_then(|v| v.as_u64()),
        Some(TOTAL as u64)
    );

    let after = client
        .get(format!("{base}/v1/pull?max=1&ack=lease"))
        .header(ROUTE_TOKEN_HEADER, route)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(after.status(), ReqStatus::NO_CONTENT);

    handle.abort();
}
