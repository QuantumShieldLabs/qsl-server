// NA-0678 invite-slot contract (D614; DOC-SRV-007). The relay half of the
// messaging epic's Slice 1.
//
// What this file proves, and the shape of each proof:
//  - the lifecycle, with the TOMBSTONE distinction: a second redemption is
//    ALREADY_USED, never NOT_FOUND. A deleted slot would report "never existed"
//    when the truth is "someone got here first" -- the interception signal.
//  - the capability compare rejects a SAME-LENGTH wrong capability. The D-0014
//    lesson: a different-length wrong value can be rejected on length alone, so
//    it proves nothing about the fold. Note this proves the ANSWER is right, not
//    that the comparison runs in constant TIME -- that property is structural and
//    read-verified, and no timing claim is made here.
//  - atomic consume under real concurrency: exactly one winner.
//  - the C3 non-regression: pushes to routes that are NOT slots behave exactly
//    as before, which is what lets Slice 2 exist before the client is rewritten.
//  - opacity: bundle and signature are stored and returned byte-identical for
//    input that is not valid anything, and neither appears in logs.
//  - both auth modes on every new route.
//  - the create-rate bucket and the slot cap, which are NOT substitutes.
//  - no /v1/invite/mint route exists.

use qsl_server::{
    app, AppState, InviteLimits, Limits, ResourceControls, ServerInfoCfg, StoreConfig,
};
use reqwest::StatusCode as ReqStatus;
use serde_json::Value;
use tokio::net::TcpListener;
use tracing::subscriber::set_default;

mod common;
use common::{await_log, capture};

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const TICKET_HEADER: &str = "X-QSL-Invite-Ticket";

async fn spawn(
    relay_token: Option<String>,
    invites: InviteLimits,
) -> (String, tokio::task::JoinHandle<()>) {
    let state = AppState::new_full(
        Limits::new(1024 * 1024, 16).unwrap_or_else(|e| panic!("{e}")),
        ResourceControls::new(64, 64, 64).unwrap_or_else(|e| panic!("{e}")),
        relay_token,
        StoreConfig::default(),
        ServerInfoCfg::default(),
        invites,
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

async fn spawn_open() -> (String, tokio::task::JoinHandle<()>) {
    spawn(None, InviteLimits::default()).await
}

fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(T[(n & 63) as usize] as char);
        }
    }
    out
}

fn unb64(s: &str) -> Vec<u8> {
    fn val(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => panic!("bad b64 byte"),
        }
    }
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        acc = (acc << 6) | u32::from(val(c));
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out
}

fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(s.as_bytes());
    let mut o = String::new();
    for b in d {
        o.push_str(&format!("{b:02x}"));
    }
    o
}

fn future_expiry() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        + 3600
}

async fn create_invite(
    client: &reqwest::Client,
    base: &str,
    invite_id: &str,
    cap: &str,
    bundle: &[u8],
    sig: &[u8],
    expiry: i64,
) -> reqwest::Response {
    client
        .post(format!("{base}/v1/invite/create"))
        .json(&serde_json::json!({
            "invite_id": invite_id,
            "cap_hash": sha256_hex(cap),
            "expiry": expiry,
            "bundle_b64": b64(bundle),
            "invite_sig_b64": b64(sig),
        }))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
}

async fn redeem(
    client: &reqwest::Client,
    base: &str,
    invite_id: &str,
    cap: &str,
) -> reqwest::Response {
    client
        .post(format!("{base}/v1/invite/redeem"))
        .json(&serde_json::json!({ "invite_id": invite_id, "cap": cap }))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
}

// ---------------------------------------------------------------- lifecycle

#[tokio::test]
async fn lifecycle_create_redeem_consume_and_second_redeem_is_already_used() {
    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    let bundle = b"OPAQUE-BUNDLE-BYTES".to_vec();
    let sig = b"OPAQUE-SIGNATURE-BYTES".to_vec();

    let created = create_invite(
        &c,
        &base,
        "inv-lifecycle",
        "cap-secret-1",
        &bundle,
        &sig,
        future_expiry(),
    )
    .await;
    assert_eq!(created.status(), ReqStatus::OK);
    let cdoc: Value = created.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert!(
        cdoc["revoke_token"].as_str().is_some_and(|t| t.len() >= 32),
        "create must return a revoke_token (F2)"
    );

    let r1 = redeem(&c, &base, "inv-lifecycle", "cap-secret-1").await;
    assert_eq!(r1.status(), ReqStatus::OK);
    let d: Value = r1.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(unb64(d["bundle_b64"].as_str().unwrap()), bundle);
    assert_eq!(unb64(d["invite_sig_b64"].as_str().unwrap()), sig);
    assert!(
        d["ticket"].as_str().is_some_and(|t| t.len() >= 32),
        "redeem must issue a one-shot handshake ticket (F3)"
    );

    // THE TOMBSTONE DISTINCTION. A deleted slot would answer NOT_FOUND here.
    let r2 = redeem(&c, &base, "inv-lifecycle", "cap-secret-1").await;
    assert_eq!(r2.status(), ReqStatus::CONFLICT);
    assert_eq!(
        r2.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_INVITE_ALREADY_USED",
        "a consumed slot must be distinguishable from one that never existed"
    );
    h.abort();
}

#[tokio::test]
async fn unknown_invite_is_not_found_not_already_used() {
    // The negative half of the tombstone claim: the two causes really are
    // different, so the assertion above is not vacuous.
    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    let r = redeem(&c, &base, "inv-never-existed", "cap-x").await;
    assert_eq!(r.status(), ReqStatus::NOT_FOUND);
    assert_eq!(
        r.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_INVITE_NOT_FOUND"
    );
    h.abort();
}

#[tokio::test]
async fn expired_invite_dies_and_returns_no_bundle() {
    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    // Create with a 1-second life, then let it lapse.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let created = create_invite(&c, &base, "inv-exp", "cap-exp", b"B", b"S", now + 1).await;
    assert_eq!(created.status(), ReqStatus::OK);
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let r = redeem(&c, &base, "inv-exp", "cap-exp").await;
    assert_eq!(r.status(), ReqStatus::GONE);
    let body = r.text().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body, "ERR_INVITE_EXPIRED");
    assert!(
        !body.contains('B'),
        "no bundle may leak on the expired path"
    );
    h.abort();
}

// ---------------------------------------------------------------- capability

#[tokio::test]
async fn same_length_wrong_capability_rejects_with_no_mutation() {
    // "cap-secret-1" vs "cap-secret-X": SAME LENGTH. A different-length value
    // could be rejected on length alone and would prove nothing about the fold.
    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    create_invite(
        &c,
        &base,
        "inv-ct",
        "cap-secret-1",
        b"BUNDLE",
        b"SIG",
        future_expiry(),
    )
    .await;

    let bad = redeem(&c, &base, "inv-ct", "cap-secret-X").await;
    assert_eq!(bad.status(), ReqStatus::FORBIDDEN);
    assert_eq!(
        bad.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_INVITE_CAP_INVALID"
    );

    // NO MUTATION: the real capability still works, so the failed attempt did
    // not consume the slot.
    let good = redeem(&c, &base, "inv-ct", "cap-secret-1").await;
    assert_eq!(good.status(), ReqStatus::OK);
    h.abort();
}

#[tokio::test]
async fn same_length_wrong_revoke_token_rejects_with_no_mutation() {
    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    let created = create_invite(
        &c,
        &base,
        "inv-rev",
        "cap-rev",
        b"BUNDLE",
        b"SIG",
        future_expiry(),
    )
    .await;
    let cdoc: Value = created.json().await.unwrap_or_else(|e| panic!("{e}"));
    let real = cdoc["revoke_token"].as_str().unwrap().to_string();
    // Flip one character, preserving length.
    let mut wrong: Vec<char> = real.chars().collect();
    wrong[0] = if wrong[0] == 'a' { 'b' } else { 'a' };
    let wrong: String = wrong.into_iter().collect();
    assert_eq!(wrong.len(), real.len());

    let bad = reqwest::Client::new()
        .post(format!("{base}/v1/invite/revoke"))
        .json(&serde_json::json!({ "invite_id": "inv-rev", "revoke_token": wrong }))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(bad.status(), ReqStatus::FORBIDDEN);
    assert_eq!(
        bad.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_INVITE_REVOKE_INVALID"
    );

    // Not mutated: the slot is still redeemable.
    assert_eq!(
        redeem(&c, &base, "inv-rev", "cap-rev").await.status(),
        ReqStatus::OK
    );
    h.abort();
}

#[tokio::test]
async fn revoke_kills_the_slot_and_is_idempotent() {
    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    let created = create_invite(
        &c,
        &base,
        "inv-kill",
        "cap-kill",
        b"BUNDLE",
        b"SIG",
        future_expiry(),
    )
    .await;
    let cdoc: Value = created.json().await.unwrap_or_else(|e| panic!("{e}"));
    let tok = cdoc["revoke_token"].as_str().unwrap().to_string();

    for _ in 0..2 {
        let r = c
            .post(format!("{base}/v1/invite/revoke"))
            .json(&serde_json::json!({ "invite_id": "inv-kill", "revoke_token": tok }))
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.status(), ReqStatus::OK, "revoke must be idempotent");
        let d: Value = r.json().await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(d["revoked"], true);
    }

    let after = redeem(&c, &base, "inv-kill", "cap-kill").await;
    assert_eq!(after.status(), ReqStatus::GONE);
    assert_eq!(
        after.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_INVITE_REVOKED"
    );
    h.abort();
}

// ---------------------------------------------------------------- atomicity

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_redemption_yields_exactly_one_winner() {
    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    create_invite(
        &c,
        &base,
        "inv-race",
        "cap-race",
        b"BUNDLE",
        b"SIG",
        future_expiry(),
    )
    .await;

    let mut tasks = Vec::new();
    for _ in 0..12 {
        let (b, cl) = (base.clone(), c.clone());
        tasks.push(tokio::spawn(async move {
            redeem(&cl, &b, "inv-race", "cap-race").await.status()
        }));
    }
    let mut ok = 0usize;
    let mut used = 0usize;
    for t in tasks {
        match t.await.unwrap_or_else(|e| panic!("{e}")) {
            ReqStatus::OK => ok += 1,
            ReqStatus::CONFLICT => used += 1,
            other => panic!("unexpected status {other}"),
        }
    }
    assert_eq!(ok, 1, "compare-and-set must yield exactly one winner");
    assert_eq!(used, 11, "every loser must see ALREADY_USED");
    h.abort();
}

// ------------------------------------------------- handshake ticket / C3

#[tokio::test]
async fn handshake_needs_the_ticket_and_the_ticket_is_one_shot() {
    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    create_invite(
        &c,
        &base,
        "inv-hs",
        "cap-hs",
        b"BUNDLE",
        b"SIG",
        future_expiry(),
    )
    .await;

    // Without a ticket: refused, even though the pusher knows invite_id.
    let no_ticket = c
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, "inv-hs")
        .body(b"handshake".to_vec())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(no_ticket.status(), ReqStatus::FORBIDDEN);
    assert_eq!(
        no_ticket.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_INVITE_TICKET_INVALID"
    );

    let d: Value = redeem(&c, &base, "inv-hs", "cap-hs")
        .await
        .json()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let ticket = d["ticket"].as_str().unwrap().to_string();

    let ok = c
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, "inv-hs")
        .header(TICKET_HEADER, &ticket)
        .body(b"handshake".to_vec())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(ok.status(), ReqStatus::OK);

    // ONE SHOT: the same ticket a second time is refused.
    let replay = c
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, "inv-hs")
        .header(TICKET_HEADER, &ticket)
        .body(b"handshake-again".to_vec())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(replay.status(), ReqStatus::FORBIDDEN);

    // Alice can still PULL her slot -- pull is deliberately ungated.
    let pull = c
        .get(format!("{base}/v1/pull?max=4"))
        .header(ROUTE_TOKEN_HEADER, "inv-hs")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(pull.status(), ReqStatus::OK);
    h.abort();
}

#[tokio::test]
async fn non_slot_routes_are_completely_unaffected() {
    // THE C3 GUARANTEE. A route the invite system never created must behave
    // exactly as it did before this lane -- no ticket, no gate, no change.
    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    let push = c
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, "ordinary-route")
        .body(b"ordinary".to_vec())
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(push.status(), ReqStatus::OK, "no ticket required off-slot");

    let pull = c
        .get(format!("{base}/v1/pull?max=1"))
        .header(ROUTE_TOKEN_HEADER, "ordinary-route")
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(pull.status(), ReqStatus::OK);
    let d: Value = pull.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(unb64(&b64(b"ordinary")), b"ordinary".to_vec());
    assert_eq!(d["items"][0]["data"].as_array().unwrap().len(), 8);
    h.abort();
}

// ---------------------------------------------------------------- opacity

#[tokio::test]
async fn bundle_is_opaque_bytes_in_bytes_out_and_never_logged() {
    let (buf, w) = capture();
    let sub = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || w.clone())
        .finish();
    let _g = set_default(sub);

    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    // Deliberately NOT valid anything: no TLV, no UTF-8, no structure. The relay
    // must neither parse nor care.
    let junk: Vec<u8> = (0..=255u8).rev().collect();
    let sig: Vec<u8> = vec![0xFF, 0x00, 0xFE, 0x01];
    create_invite(
        &c,
        &base,
        "inv-opaque",
        "cap-op",
        &junk,
        &sig,
        future_expiry(),
    )
    .await;
    let d: Value = redeem(&c, &base, "inv-opaque", "cap-op")
        .await
        .json()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        unb64(d["bundle_b64"].as_str().unwrap()),
        junk,
        "byte-identical"
    );
    assert_eq!(unb64(d["invite_sig_b64"].as_str().unwrap()), sig);

    // NA-0687: this is ENG-0091 instance 1, measured RED at full parallelism. Await
    // the relay's redacted-id line BEFORE aborting -- the two absence assertions below
    // must be measured against a populated buffer, never a racy empty one.
    let logged = await_log(&buf, "channel_id=").await;
    h.abort();
    assert!(
        !logged.contains("inv-opaque"),
        "raw invite_id must not be logged"
    );
    assert!(!logged.contains(&b64(&junk)), "bundle must not be logged");
    assert!(logged.contains("channel_id="), "redacted id must be logged");
}

// ---------------------------------------------------------------- auth modes

#[tokio::test]
async fn every_invite_route_is_gated_on_a_bearer_relay() {
    let (base, h) = spawn(Some("topsecret".to_string()), InviteLimits::default()).await;
    let c = reqwest::Client::new();
    for (path, body) in [
        (
            "/v1/invite/create",
            serde_json::json!({"invite_id":"x","cap_hash":"y","expiry":future_expiry(),"bundle_b64":"QQ","invite_sig_b64":"QQ"}),
        ),
        (
            "/v1/invite/redeem",
            serde_json::json!({"invite_id":"x","cap":"y"}),
        ),
        (
            "/v1/invite/revoke",
            serde_json::json!({"invite_id":"x","revoke_token":"y"}),
        ),
    ] {
        let r = c
            .post(format!("{base}{path}"))
            .json(&body)
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.status(), ReqStatus::UNAUTHORIZED, "{path} must be gated");
        // Plain ERR_UNAUTHORIZED -- the new routes do NOT adopt the server-info
        // probe body (DOC-SRV-006 rule 4).
        assert_eq!(
            r.text().await.unwrap_or_else(|e| panic!("{e}")),
            "ERR_UNAUTHORIZED",
            "{path} must not adopt the capability-probe body"
        );
    }
    // With the token, create works.
    let ok = c
        .post(format!("{base}/v1/invite/create"))
        .header("Authorization", "Bearer topsecret")
        .json(&serde_json::json!({
            "invite_id":"inv-auth","cap_hash":sha256_hex("cap"),"expiry":future_expiry(),
            "bundle_b64":b64(b"B"),"invite_sig_b64":b64(b"S")}))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(ok.status(), ReqStatus::OK);
    h.abort();
}

// ------------------------------------------------- rate bucket and slot cap

#[tokio::test]
async fn create_rate_bucket_exhausts_and_creates_no_slot() {
    // F6: availability of invite-create is a security property. The bucket and
    // the cap are BOTH required and are not substitutes -- this is the bucket.
    let limits = InviteLimits::new(256, 16 * 1024, 259_200, 2, 0).unwrap_or_else(|e| panic!("{e}"));
    let (base, h) = spawn(None, limits).await;
    let c = reqwest::Client::new();
    let e = future_expiry();
    assert_eq!(
        create_invite(&c, &base, "r1", "c1", b"B", b"S", e)
            .await
            .status(),
        ReqStatus::OK
    );
    assert_eq!(
        create_invite(&c, &base, "r2", "c2", b"B", b"S", e)
            .await
            .status(),
        ReqStatus::OK
    );

    let limited = create_invite(&c, &base, "r3", "c3", b"B", b"S", e).await;
    assert_eq!(limited.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        limited.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_RATE_LIMITED"
    );
    // NO SLOT CREATED by the refused call.
    assert_eq!(
        redeem(&c, &base, "r3", "c3").await.status(),
        ReqStatus::NOT_FOUND,
        "a rate-limited create must not have stored anything"
    );
    h.abort();
}

#[tokio::test]
async fn slot_cap_rejects_and_never_evicts() {
    let limits =
        InviteLimits::new(2, 16 * 1024, 259_200, 4096, 0).unwrap_or_else(|e| panic!("{e}"));
    let (base, h) = spawn(None, limits).await;
    let c = reqwest::Client::new();
    let e = future_expiry();
    assert_eq!(
        create_invite(&c, &base, "cap1", "c1", b"B", b"S", e)
            .await
            .status(),
        ReqStatus::OK
    );
    assert_eq!(
        create_invite(&c, &base, "cap2", "c2", b"B", b"S", e)
            .await
            .status(),
        ReqStatus::OK
    );

    let full = create_invite(&c, &base, "cap3", "c3", b"B", b"S", e).await;
    assert_eq!(full.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        full.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_INVITE_CAP_FULL"
    );
    // NEVER EVICTS: both earlier slots survive. An eviction path would let an
    // attacker delete other people's invites -- worse than the denial.
    assert_eq!(
        redeem(&c, &base, "cap1", "c1").await.status(),
        ReqStatus::OK
    );
    assert_eq!(
        redeem(&c, &base, "cap2", "c2").await.status(),
        ReqStatus::OK
    );
    h.abort();
}

#[tokio::test]
async fn oversize_bundle_is_refused() {
    let limits = InviteLimits::new(256, 64, 259_200, 4096, 0).unwrap_or_else(|e| panic!("{e}"));
    let (base, h) = spawn(None, limits).await;
    let c = reqwest::Client::new();
    let big = vec![7u8; 65];
    let r = create_invite(&c, &base, "inv-big", "cap", &big, b"S", future_expiry()).await;
    assert_eq!(r.status(), ReqStatus::PAYLOAD_TOO_LARGE);
    assert_eq!(
        r.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_INVITE_TOO_LARGE"
    );
    h.abort();
}

// ---------------------------------------------------------------- route set

#[tokio::test]
async fn there_is_no_mint_route() {
    // F1 deleted it. The client mints the capability and uploads only its hash,
    // so no relay-side path ever holds a capability in plaintext.
    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    for path in ["/v1/invite/mint", "/v1/invite/capability", "/v1/mint"] {
        let r = c
            .post(format!("{base}{path}"))
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            r.status(),
            ReqStatus::NOT_FOUND,
            "{path} must not exist -- F1 removed relay-side minting"
        );
    }
    h.abort();
}

#[tokio::test]
async fn server_info_advertises_invite_v1_additively() {
    let (base, h) = spawn_open().await;
    let c = reqwest::Client::new();
    let doc: Value = c
        .get(format!("{base}/v1/server-info"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
        .json()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let api = doc["api"].as_array().unwrap();
    assert!(api.iter().any(|v| v == "invite_v1"));
    // Additive: every pre-existing entry survives, in order.
    for (i, want) in ["push_v1", "pull_v1", "pull_ack_lease_v1"]
        .iter()
        .enumerate()
    {
        assert_eq!(&api[i], want, "pre-existing api entries must not move");
    }
    assert!(doc["invite"]["max_expiry_secs"].is_number());
    assert!(doc["invite"]["max_slots"].is_number());
    assert!(doc["limits"]["max_invite_bundle_bytes"].is_number());
    h.abort();
}
