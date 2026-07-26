// NA-0642 restart-durability proof: the relay process is HARD-KILLED (SIGKILL
// via Child::kill on unix — no graceful shutdown, no flush-on-exit) between the
// push 200 and the restart, so surviving the restart demonstrates PROCESS-CRASH
// durability, not a graceful flush. Same discipline for the
// crash-between-pull-and-ack case: the server dies mid-lease and the leased
// message must reappear after lease expiry.
//
// ⚠ COMMENT CORRECTED at NA-0678 (D614 C2/F4). This header previously claimed
// the file demonstrates "the synchronous=FULL fsync guarantee". It cannot, and
// the correction matters because that claim was cited as the project's proof of
// "a 200 means fsynced". SIGKILL destroys a PROCESS, not the OS page cache:
// writes that reached the kernel survive a process kill and are visible to the
// next process that opens the file, so synchronous=FULL and synchronous=OFF are
// INDISTINGUISHABLE to any process-kill test. Measured, not argued: this suite
// passes 3/3 with the pragma set to OFF.
//
// The tests below are correct and valuable for what they actually prove, and
// are deliberately unchanged. The fsync claim is discharged instead by
// tests/na0678_invite_durability.rs, which counts real fsync syscalls and
// asserts the fsync precedes the 200 on the wire.

use serde::Deserialize;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const MSG_ID_HEADER: &str = "X-Msg-Id";

#[derive(Deserialize)]
struct PullItem {
    id: String,
    data: Vec<u8>,
}

#[derive(Deserialize)]
struct PullResp {
    items: Vec<PullItem>,
}

#[derive(Deserialize)]
struct AckResp {
    acked: usize,
}

struct Relay {
    child: Child,
    base: String,
}

impl Relay {
    fn spawn(store_path: &str, extra_envs: &[(&str, &str)]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_qsl-server"));
        command
            .env_clear()
            .env("RUST_LOG", "info")
            .env("BIND_ADDR", "127.0.0.1")
            .env("PORT", "0")
            .env("STORE_PATH", store_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (name, value) in extra_envs {
            command.env(name, value);
        }
        let mut child = command.spawn().unwrap_or_else(|e| panic!("{e}"));
        let stdout = child.stdout.take().unwrap_or_else(|| panic!("no stdout"));
        let (tx, rx) = mpsc::channel::<String>();
        // Keep draining stdout for the process lifetime so logging never
        // blocks on a full pipe; only the first "listening on" line matters.
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

    /// SIGKILL — the process gets no chance to flush or shut down cleanly.
    fn hard_kill(mut self) {
        self.child.kill().unwrap_or_else(|e| panic!("{e}"));
        self.child.wait().unwrap_or_else(|e| panic!("{e}"));
    }
}

fn temp_store(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "na0642-{}-{}-{}",
        tag,
        std::process::id(),
        uuid_ish()
    ));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    dir.join("relay.db").to_string_lossy().into_owned()
}

fn uuid_ish() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

async fn push(
    client: &reqwest::Client,
    base: &str,
    route: &str,
    msg_id: &str,
    body: Vec<u8>,
) -> reqwest::Response {
    client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, route)
        .header(MSG_ID_HEADER, msg_id)
        .body(body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
}

async fn pull(
    client: &reqwest::Client,
    base: &str,
    route: &str,
    max: usize,
    lease: bool,
) -> reqwest::Response {
    let url = if lease {
        format!("{base}/v1/pull?max={max}&ack=lease")
    } else {
        format!("{base}/v1/pull?max={max}")
    };
    client
        .get(url)
        .header(ROUTE_TOKEN_HEADER, route)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"))
}

#[tokio::test(flavor = "current_thread")]
async fn pushed_message_survives_hard_kill_and_restart() {
    let store = temp_store("restart");
    let route = "NA0642_RESTART_ROUTE";
    let payload: Vec<u8> = (0..32_768u32).map(|i| (i % 251) as u8).collect();

    let relay = Relay::spawn(&store, &[]);
    let client = reqwest::Client::new();
    let accepted = push(
        &client,
        &relay.base,
        route,
        "NA0642_RESTART_MSG",
        payload.clone(),
    )
    .await;
    assert_eq!(accepted.status(), reqwest::StatusCode::OK);

    // HARD KILL immediately after the 200: no graceful shutdown, no flush.
    relay.hard_kill();

    let relay = Relay::spawn(&store, &[]);
    let delivered = pull(&client, &relay.base, route, 2, false).await;
    assert_eq!(delivered.status(), reqwest::StatusCode::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0642_RESTART_MSG");
    assert_eq!(body.items[0].data, payload, "payload not byte-identical");

    // Delete-on-deliver still holds after the durable round-trip.
    let after = pull(&client, &relay.base, route, 1, false).await;
    assert_eq!(after.status(), reqwest::StatusCode::NO_CONTENT);

    relay.hard_kill();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&store).parent().unwrap());
}

#[tokio::test(flavor = "current_thread")]
async fn leased_message_survives_hard_kill_and_reappears_after_lease_expiry() {
    let store = temp_store("lease-crash");
    let route = "NA0642_LEASE_CRASH_ROUTE";
    let lease_env = [("PULL_LEASE_SECS", "5")];

    let relay = Relay::spawn(&store, &lease_env);
    let client = reqwest::Client::new();
    let accepted = push(
        &client,
        &relay.base,
        route,
        "NA0642_LEASE_CRASH_MSG",
        b"crash-window-payload".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), reqwest::StatusCode::OK);

    let leased = pull(&client, &relay.base, route, 1, true).await;
    assert_eq!(leased.status(), reqwest::StatusCode::OK);
    let leased_body: PullResp = leased.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(leased_body.items.len(), 1);

    // HARD KILL between the acknowledged-pull and the ack: the crash window
    // the design exists for. The message must NOT be lost.
    relay.hard_kill();

    let relay = Relay::spawn(&store, &lease_env);
    // The lease itself survived the restart: while it is live the message
    // stays in-flight and invisible.
    let still_leased = pull(&client, &relay.base, route, 1, true).await;
    assert_eq!(still_leased.status(), reqwest::StatusCode::NO_CONTENT);

    // After lease expiry the message reappears, byte-identical.
    tokio::time::sleep(Duration::from_secs(6)).await;
    let redelivered = pull(&client, &relay.base, route, 1, true).await;
    assert_eq!(redelivered.status(), reqwest::StatusCode::OK);
    let body: PullResp = redelivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].id, "NA0642_LEASE_CRASH_MSG");
    assert_eq!(body.items[0].data.as_slice(), b"crash-window-payload");

    // This time the ack lands: the message is gone for good.
    let acked = client
        .post(format!("{}/v1/pull/ack", relay.base))
        .header(ROUTE_TOKEN_HEADER, route)
        .json(&serde_json::json!({ "ids": ["NA0642_LEASE_CRASH_MSG"] }))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(acked.status(), reqwest::StatusCode::OK);
    let acked: AckResp = acked.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(acked.acked, 1);
    let drained = pull(&client, &relay.base, route, 1, true).await;
    assert_eq!(drained.status(), reqwest::StatusCode::NO_CONTENT);

    relay.hard_kill();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&store).parent().unwrap());
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_delivery_does_not_resurrect_after_restart() {
    // Negative control: a message delivered via the legacy delete-on-pull
    // contract must NOT come back after a hard kill + restart (the store
    // must be durable for queued messages, forgetful for delivered ones).
    let store = temp_store("no-resurrect");
    let route = "NA0642_NO_RESURRECT_ROUTE";

    let relay = Relay::spawn(&store, &[]);
    let client = reqwest::Client::new();
    let accepted = push(
        &client,
        &relay.base,
        route,
        "NA0642_NO_RESURRECT_MSG",
        b"delivered-then-gone".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), reqwest::StatusCode::OK);

    let delivered = pull(&client, &relay.base, route, 1, false).await;
    assert_eq!(delivered.status(), reqwest::StatusCode::OK);

    relay.hard_kill();

    let relay = Relay::spawn(&store, &[]);
    let after = pull(&client, &relay.base, route, 1, false).await;
    assert_eq!(after.status(), reqwest::StatusCode::NO_CONTENT);

    relay.hard_kill();
    let _ = std::fs::remove_dir_all(std::path::Path::new(&store).parent().unwrap());
}
