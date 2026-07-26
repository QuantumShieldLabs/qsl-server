// NA-0678 (D614 F4): the "200 means fsynced" claim, proved by an instrument that
// can actually observe fsync.
//
// ⚠ WHY THIS FILE EXISTS AT ALL. `tests/na0642_durability_restart.rs` is widely
// cited as the proof of that claim. It cannot be: it SIGKILLs the relay and
// restarts it, and SIGKILL destroys a PROCESS, not the OS page cache. Writes
// that reached the kernel survive a process kill and are visible to the next
// process that opens the file, so `synchronous=FULL` and `synchronous=OFF` are
// INDISTINGUISHABLE to any process-kill test. Measured during the D614 census:
// that suite passes 3/3 with the pragma set to OFF.
//
// That test is still valuable and is deliberately kept -- it proves PROCESS-crash
// durability, which is a real property. It simply does not prove this one.
//
// The instrument here counts real `fsync`/`fdatasync` syscalls attributable to an
// accepted invite create, and asserts the ordering that makes the claim mean
// something: the fsync must complete BEFORE the 200 reaches the socket.
//
// ⚠ SKIP DISCIPLINE. When `strace` is unavailable this test SKIPS WITH A STATED
// REASON naming what it could not examine. A silent skip is a vacuous pass --
// indistinguishable from a passing gate -- and is exactly the defect class the
// standing rules exist to prevent. The `synchronous=OFF` negative arm cannot be
// built from inside the test binary (it needs a differently-compiled relay), so
// it is discharged by a recorded local run in the lane's as-built evidence.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn have_strace() -> bool {
    Command::new("strace")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "na0678-dur-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{e}"));
    d
}

/// Run the real relay binary under strace, perform `n` invite creates, and
/// return (fsync syscalls observed, whether an fsync preceded the last 200).
fn measure(n: usize, tag: &str) -> (usize, bool) {
    let dir = temp_dir(tag);
    let trace = dir.join("trace.log");
    let store = dir.join("relay.db");

    let mut child = Command::new("strace")
        .args([
            "-f",
            "-tt",
            "-e",
            "trace=fsync,fdatasync,write,writev,sendto",
            "-o",
        ])
        .arg(&trace)
        .arg(env!("CARGO_BIN_EXE_qsl-server"))
        .env_clear()
        .env("RUST_LOG", "info")
        .env("BIND_ADDR", "127.0.0.1")
        .env("PORT", "0")
        .env("STORE_PATH", &store)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn strace: {e}"));

    let stdout = child.stdout.take().unwrap_or_else(|| panic!("no stdout"));
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut sent = false;
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if !sent {
                if let Some(i) = line.find("listening on ") {
                    let _ = tx.send(line[i + "listening on ".len()..].trim().to_string());
                    sent = true;
                }
            }
        }
    });
    let addr = rx
        .recv_timeout(Duration::from_secs(20))
        .unwrap_or_else(|e| panic!("relay never reported a listen address: {e}"));

    let before = std::fs::read_to_string(&trace).unwrap_or_default();
    let before_fsync = before.matches("fsync(").count() + before.matches("fdatasync(").count();

    let expiry = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
        + 3600;
    for i in 0..n {
        let body = format!(
            r#"{{"invite_id":"dur-{i}","cap_hash":"{}","expiry":{expiry},"bundle_b64":"QUJD","invite_sig_b64":"WFla"}}"#,
            sha256_hex("cap")
        );
        let out = Command::new("curl")
            .args([
                "-sS",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                "-X",
                "POST",
                &format!("http://{addr}/v1/invite/create"),
                "-H",
                "Content-Type: application/json",
                "--data-binary",
                &body,
            ])
            .output()
            .unwrap_or_else(|e| panic!("curl: {e}"));
        let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
        assert_eq!(code, "200", "invite create must be accepted (got {code})");
    }
    std::thread::sleep(Duration::from_millis(600));
    let _ = child.kill();
    let _ = child.wait();

    let after = std::fs::read_to_string(&trace).unwrap_or_default();
    let after_fsync = after.matches("fsync(").count() + after.matches("fdatasync(").count();

    // Ordering: the LAST fsync must appear before the LAST 200-carrying write.
    let lines: Vec<&str> = after.lines().collect();
    let last_fsync = lines
        .iter()
        .rposition(|l| l.contains("fsync(") || l.contains("fdatasync("));
    let last_200 = lines.iter().rposition(|l| l.contains("HTTP/1.1 200 OK"));
    let ordered = match (last_fsync, last_200) {
        (Some(f), Some(r)) => f < r,
        // No 200 line visible in the trace (buffering) -> report unordered rather
        // than claiming a property we did not observe.
        _ => false,
    };
    let _ = std::fs::remove_dir_all(&dir);
    (after_fsync.saturating_sub(before_fsync), ordered)
}

#[test]
fn accepted_invite_create_is_fsynced_before_the_200() {
    if !have_strace() {
        // NOT a silent skip. Name the tool, the property, and where the missing
        // coverage is discharged instead.
        println!(
            "SKIP na0678 durability instrument: `strace` is not available on this host, \
             so NO fsync syscall could be observed. NOT EXAMINED: whether an accepted \
             POST /v1/invite/create performs an fsync before its 200 reaches the socket. \
             This property is discharged by the recorded both-arms local run in the \
             lane's as-built evidence (D614 F4); it is NOT proven by this run."
        );
        return;
    }

    let (zero_arm, _) = measure(0, "zero");
    let (five_arm, ordered) = measure(5, "five");

    // The instrument must be able to return a LOW number too, or a high count
    // proves nothing about attribution.
    assert_eq!(
        zero_arm, 0,
        "with no creates, no create-attributable fsync may be counted (got {zero_arm})"
    );
    assert!(
        five_arm >= 5,
        "five accepted creates must produce at least five fsyncs (got {five_arm})"
    );
    assert!(
        ordered,
        "the fsync must complete BEFORE the 200 reaches the socket -- otherwise \
         '200 means durable' is false"
    );
    println!(
        "na0678 durability: EXAMINED 0-create and 5-create runs under strace; \
         fsync delta 0 and {five_arm}; fsync-before-200 ordering observed."
    );
}
