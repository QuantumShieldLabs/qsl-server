// REVIEW-relay-server C2 (SR-19 delta symbol JB: per-route JSON body caps in invite_create/redeem/revoke, ack_messages; base 0c04fa47).
// RED at 0c04fa47: oversize JSON bodies are parsed, hashed and even stored. GREEN: 413 before any parse.
use qsl_server::{app, AppState, Limits};
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

async fn post(c: &reqwest::Client, url: String, body: String) -> (u16, String) {
    let r = c
        .post(url)
        .header("x-qsl-route-token", "review-c2")
        .body(body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let s = r.status().as_u16();
    let t = r.text().await.unwrap_or_else(|e| panic!("{e}"));
    (s, t)
}

#[tokio::test]
async fn review_c2_json_routes_refuse_oversize_bodies_before_parse() {
    let (base, handle) = spawn(Limits {
        max_body_bytes: 65536,
        max_queue_depth: 8,
    })
    .await;
    let c = reqwest::Client::new();
    let big = "a".repeat(1024 * 1024);
    let q = "b".repeat(256 * 1024);

    // Positive controls first: ordinary small bodies are unaffected.
    let (s, t) = post(
        &c,
        format!("{base}/v1/invite/redeem"),
        r#"{"invite_id":"nope","cap":"x"}"#.to_string(),
    )
    .await;
    println!("REVIEW_C2 control_redeem status={s} body={t}");
    assert_eq!((s, t.as_str()), (404, "ERR_INVITE_NOT_FOUND"));
    let (s, _) = post(&c, format!("{base}/v1/invite/create"),
        r#"{"invite_id":"small-ok","cap_hash":"c","expiry":4102444800,"bundle_b64":"AA","invite_sig_b64":"AA"}"#.to_string()).await;
    println!("REVIEW_C2 control_create status={s}");
    assert_eq!(s, 200);

    let (rs, rt) = post(
        &c,
        format!("{base}/v1/invite/redeem"),
        format!(r#"{{"invite_id":"{big}","cap":"x"}}"#),
    )
    .await;
    let (vs, vt) = post(
        &c,
        format!("{base}/v1/invite/revoke"),
        format!(r#"{{"invite_id":"{big}","revoke_token":"x"}}"#),
    )
    .await;
    let (as_, at) = post(
        &c,
        format!("{base}/v1/pull/ack"),
        format!(r#"{{"ids":["{q}","{q}","{q}","{q}","{q}","{q}"]}}"#),
    )
    .await;
    let (cs, ct) = post(&c, format!("{base}/v1/invite/create"),
        format!(r#"{{"invite_id":"{big}","cap_hash":"c","expiry":4102444800,"bundle_b64":"AA","invite_sig_b64":"AA"}}"#)).await;
    println!(
        "REVIEW_C2 redeem_1MiB status={rs} body={}",
        &rt[..rt.len().min(40)]
    );
    println!(
        "REVIEW_C2 revoke_1MiB status={vs} body={}",
        &vt[..vt.len().min(40)]
    );
    println!(
        "REVIEW_C2 ack_1.5MiB status={as_} body={}",
        &at[..at.len().min(40)]
    );
    println!(
        "REVIEW_C2 create_1MiB status={cs} body={}",
        &ct[..ct.len().min(40)]
    );
    handle.abort();
    assert_eq!((rs, rt.as_str()), (413, "ERR_TOO_LARGE"));
    assert_eq!((vs, vt.as_str()), (413, "ERR_TOO_LARGE"));
    assert_eq!((as_, at.as_str()), (413, "ERR_TOO_LARGE"));
    assert_eq!((cs, ct.as_str()), (413, "ERR_INVITE_TOO_LARGE"));
}
