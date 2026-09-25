// REVIEW-relay-server C1 (SR-19 delta symbol PB: store::Store::pull byte budget; base qsl-server 0c04fa47).
// RED at 0c04fa47: one pull returns the whole queue. GREEN: at most max(16 MiB, max_body_bytes) raw bytes per pull
// (FINISH_SCAN_BATCH x MAX_BODY_BYTES_CEILING; RULING R4(a) raised it from 8 MiB).
use qsl_server::{app, AppState, Limits, MAX_BODY_BYTES_CEILING};
use tokio::net::TcpListener;

async fn spawn(limits: Limits) -> (String, tokio::task::JoinHandle<()>) {
    let state = AppState::new_with_auth(limits, None);
    let router = app(state);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let addr = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    });
    (format!("http://{addr}"), handle)
}

fn raw_len(v: &serde_json::Value) -> (usize, usize) {
    let items = v["items"].as_array().unwrap_or_else(|| panic!("no items"));
    let raw = items
        .iter()
        .map(|it| it["data"].as_array().map(|a| a.len()).unwrap_or(0))
        .sum();
    (items.len(), raw)
}

#[tokio::test]
async fn review_c1_pull_is_bounded_by_a_byte_budget_and_loses_nothing() {
    let (base, handle) = spawn(Limits {
        max_body_bytes: 256 * 1024,
        max_queue_depth: 96,
    })
    .await;
    let client = reqwest::Client::new();
    // Uniform-looking bytes, as ciphertext is.
    let msg: Vec<u8> = (0..(256 * 1024) as u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    for _ in 0..96 {
        let r = client
            .post(format!("{base}/v1/push"))
            .header("x-qsl-route-token", "review-c1")
            .body(msg.clone())
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.status(), 200);
    }
    let r = client
        .get(format!("{base}/v1/pull?max=96&ack=lease"))
        .header("x-qsl-route-token", "review-c1")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r.status(), 200);
    let body = r.bytes().await.unwrap_or_else(|e| panic!("{e}"));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_else(|e| panic!("{e}"));
    let (n, raw) = raw_len(&v);
    println!(
        "REVIEW_C1 first_pull items={n} raw_bytes={raw} response_bytes={} ratio={:.3}",
        body.len(),
        body.len() as f64 / raw as f64
    );
    // Second pull: the items the budget left behind are still there (nothing lost).
    let r2 = client
        .get(format!("{base}/v1/pull?max=96&ack=lease"))
        .header("x-qsl-route-token", "review-c1")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let s2 = r2.status();
    println!("REVIEW_C1 second_pull status={s2}");
    handle.abort();
    assert!(n >= 1);
    assert!(
        raw <= 16 * 1024 * 1024,
        "one pull returned {raw} raw bytes ({} response bytes), above the 16 MiB pull budget",
        body.len()
    );
    assert_eq!(
        s2, 200,
        "items left behind by the budget must remain pullable"
    );
}

// RULING R4(a): qsc's invite-finish scan pulls batches of FINISH_SCAN_BATCH (16) frames and reads a batch SHORTER
// than it asked for as an exhausted mailbox. At the source defaults, a 16-frame pull of ceiling-size bodies must
// come back whole, in ONE response, or that scan stalls behind the frames the budget left queued.
#[tokio::test]
async fn review_c1_finish_scan_batch_of_16_at_the_ceiling_body_size_is_never_cut() {
    let (base, handle) = spawn(Limits::default()).await;
    let client = reqwest::Client::new();
    let msg: Vec<u8> = (0..MAX_BODY_BYTES_CEILING as u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    for _ in 0..16 {
        let r = client
            .post(format!("{base}/v1/push"))
            .header("x-qsl-route-token", "review-c1-finish")
            .body(msg.clone())
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.status(), 200);
    }
    let r = client
        .get(format!("{base}/v1/pull?max=16&ack=lease"))
        .header("x-qsl-route-token", "review-c1-finish")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r.status(), 200);
    let body = r.bytes().await.unwrap_or_else(|e| panic!("{e}"));
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_else(|e| panic!("{e}"));
    let (n, raw) = raw_len(&v);
    println!(
        "REVIEW_C1_FINISH first_pull items={n} raw_bytes={raw} response_bytes={} ratio={:.3}",
        body.len(),
        body.len() as f64 / raw as f64
    );
    // Nothing may be left behind for a second pull to find.
    let r2 = client
        .get(format!("{base}/v1/pull?max=16&ack=lease"))
        .header("x-qsl-route-token", "review-c1-finish")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let s2 = r2.status();
    println!("REVIEW_C1_FINISH second_pull status={s2}");
    handle.abort();
    assert_eq!(
        n, 16,
        "a 16-frame pull of ceiling-size bodies returned {n} items; qsc's finish scan reads a short batch as an empty mailbox"
    );
    assert_eq!(raw, 16 * MAX_BODY_BYTES_CEILING);
    assert_eq!(s2, 204, "the 16-frame batch must leave nothing behind");
}
