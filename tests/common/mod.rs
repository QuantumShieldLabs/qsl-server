#![allow(dead_code)]
//! Shared log-capture test support.
//!
//! NA-0687 / D621 — ENG-0091 + ENG-0065, one defect: **a log-capture assertion that
//! reads its buffer without synchronising on the writer having emitted.**
//!
//! Twelve sites in this repository installed a `tracing` subscriber over an
//! `Arc<Mutex<Vec<u8>>>`, drove the relay over HTTP, then read the buffer and
//! asserted, with nothing ordering the server task's emit before the read. Both
//! directions of that mistake are real:
//!
//! * a **positive** assertion (*a line is present*) fails when the emit loses the
//!   race — the merge-blocking flake, measured RED here on 2026-07-29 at
//!   `RUST_TEST_THREADS` unset, run 3 of 5, on both of ENG-0091's named instances;
//! * a **negative** assertion (*a line is absent*) passes **vacuously** when the
//!   read happens before the buffer is populated. A green absence-check over an
//!   empty buffer proves nothing. That direction can never fail, which is exactly
//!   why it is the more dangerous half.
//!
//! The rule this module exists to enforce: **synchronise, then assert** — and every
//! absence assertion runs only after a positive sentinel from the same operation has
//! been awaited, so absence is measured against a demonstrably populated buffer.
//!
//! ⚠ ORDERING IS LOAD-BEARING. Ten of the twelve sites called `handle.abort()` and
//! then read. On a current-thread runtime `abort()` guarantees a not-yet-emitted line
//! will **never** be emitted, so a wait placed after the abort could not succeed — it
//! would convert a flaky failure into a deterministic timeout. Await the sentinel
//! FIRST, then abort, then assert.
//!
//! ⚠ The 5 s deadline and 50 ms poll are **derived, not invented**: they are the
//! readiness idiom this project already uses (`qsl-protocol`
//! `qsl/qsl-client/qsc/tests/common/mod.rs`). A lane adopts the vocabulary the tree
//! already uses.
//!
//! ⚠ `tokio`'s `time` feature is **not declared by this crate** — it arrives
//! transitively via `axum` and `reqwest`. That is why `tokio::time::sleep` compiles
//! here with no `Cargo.toml` change. If a dependency ever stops enabling it, the
//! fallback that needs no feature at all is `std::time::Instant` +
//! `tokio::task::yield_now()`, at the cost of spinning a core for the whole wait.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long `try_await_log` waits for its needle before reporting a timeout.
pub const LOG_WAIT_DEADLINE: Duration = Duration::from_secs(5);
/// How often it re-reads the buffer while waiting.
pub const LOG_WAIT_POLL: Duration = Duration::from_millis(50);

/// The captured-log buffer. Identical to the type all ten hand-rolled copies used.
pub type LogBuf = Arc<Mutex<Vec<u8>>>;

/// The `tracing` writer half of a capture. Cloned per `make_writer` call, exactly as
/// the hand-rolled writers were.
#[derive(Clone)]
pub struct CaptureWriter(LogBuf);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|e| panic!("{e}"))
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Build a capture: the buffer to read from, and the writer to hand to
/// `tracing_subscriber::fmt().with_writer(move || w.clone())`.
pub fn capture() -> (LogBuf, CaptureWriter) {
    let buf: LogBuf = Arc::new(Mutex::new(Vec::new()));
    (buf.clone(), CaptureWriter(buf))
}

/// Read the buffer as text. Lossy, matching the ten sites that read with
/// `String::from_utf8_lossy`; see the lane's side-effect inventory for the two sites
/// that read strictly before this consolidation and why lossy cannot mask a needle.
pub fn log_text(buf: &LogBuf) -> String {
    let bytes = buf.lock().unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8_lossy(&bytes).to_string()
}

/// Why a wait ended without its needle.
#[derive(Debug)]
pub enum LogWaitError {
    /// ⚠ The buffer's SIZE is part of the error on purpose. A timeout over an
    /// **empty** buffer and a timeout over a **populated** one are different
    /// defects — nothing emitted at all, versus the wrong thing emitted — and they
    /// must not be reported by the same words. Diagnosing them apart must not
    /// require a second run.
    Timeout {
        needle: String,
        waited_ms: u64,
        bytes: usize,
        lines: usize,
    },
}

impl std::fmt::Display for LogWaitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogWaitError::Timeout {
                needle,
                waited_ms,
                bytes,
                lines,
            } => write!(
                f,
                "LOG_SYNC_TIMEOUT: needle {needle:?} not observed within {waited_ms}ms \
                 (buffer {bytes} bytes, {lines} lines)"
            ),
        }
    }
}

/// Wait for `needle` to appear in the captured log, up to `LOG_WAIT_DEADLINE`.
///
/// Returns the captured text at the moment the needle was observed, so a caller
/// asserts against **one** snapshot rather than re-reading a buffer that keeps
/// growing underneath it.
pub async fn try_await_log(buf: &LogBuf, needle: &str) -> Result<String, LogWaitError> {
    let start = Instant::now();
    loop {
        let text = log_text(buf);
        if text.contains(needle) {
            return Ok(text);
        }
        if start.elapsed() >= LOG_WAIT_DEADLINE {
            return Err(LogWaitError::Timeout {
                needle: needle.to_string(),
                waited_ms: start.elapsed().as_millis() as u64,
                bytes: text.len(),
                lines: text.lines().count(),
            });
        }
        tokio::time::sleep(LOG_WAIT_POLL).await;
    }
}

/// The everyday call site: wait for the operation's own log line, then hand back the
/// text to assert against. Panics with a NAMED message naming the needle, the wait
/// and the size of the buffer examined — never a bare assertion over a maybe-empty
/// buffer.
pub async fn await_log(buf: &LogBuf, needle: &str) -> String {
    match try_await_log(buf, needle).await {
        Ok(text) => text,
        Err(e) => panic!("{e}"),
    }
}

/// Await EVERY needle a site asserts positively, in order, and return the snapshot
/// taken once the last one arrived.
///
/// ⚠ Awaiting only the first needle is not enough. A site that asserts on both a push
/// line and a pull line has two emits to lose, and a wait on the first says nothing
/// about the second. Because the buffer only ever grows, the snapshot returned by the
/// final wait contains every needle awaited before it — so the assertions that follow
/// read ONE snapshot in which all of them are present.
pub async fn await_logs(buf: &LogBuf, needles: &[&str]) -> String {
    assert!(
        !needles.is_empty(),
        "await_logs with no needles would synchronise on nothing and report a pass \
         over an unexamined buffer"
    );
    let mut text = String::new();
    for needle in needles {
        text = await_log(buf, needle).await;
    }
    text
}

// ---------------------------------------------------------------- the control gate
//
// A fix whose control cannot fire has fixed nothing. The gate below makes the race
// DETERMINISTIC instead of scheduler-dependent: bytes written while it is closed are
// staged out of sight, so a reader sees exactly what a reader that lost the race
// would see. Releasing it reveals them.

struct GateState {
    released: bool,
    staging: Vec<u8>,
}

/// A writer that withholds what it is given until the gate is released.
#[derive(Clone)]
pub struct GatedWriter {
    state: Arc<Mutex<GateState>>,
    visible: LogBuf,
}

impl std::io::Write for GatedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // Lock order state -> visible, the same order `GateHandle::release` takes,
        // so the two cannot deadlock against each other.
        let mut st = self.state.lock().unwrap_or_else(|e| panic!("{e}"));
        if st.released {
            self.visible
                .lock()
                .unwrap_or_else(|e| panic!("{e}"))
                .extend_from_slice(buf);
        } else {
            st.staging.extend_from_slice(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Releases a `GatedWriter`, revealing everything staged while it was closed.
#[derive(Clone)]
pub struct GateHandle {
    state: Arc<Mutex<GateState>>,
    visible: LogBuf,
}

impl GateHandle {
    /// Idempotent: releasing twice reveals the staged bytes once.
    pub fn release(&self) {
        let mut st = self.state.lock().unwrap_or_else(|e| panic!("{e}"));
        if !st.released {
            st.released = true;
            let staged = std::mem::take(&mut st.staging);
            self.visible
                .lock()
                .unwrap_or_else(|e| panic!("{e}"))
                .extend_from_slice(&staged);
        }
    }
}

/// Build a gated capture: the buffer a reader sees, the writer to install, and the
/// handle that decides when what was written becomes visible.
pub fn gated_capture() -> (LogBuf, GatedWriter, GateHandle) {
    let visible: LogBuf = Arc::new(Mutex::new(Vec::new()));
    let state = Arc::new(Mutex::new(GateState {
        released: false,
        staging: Vec::new(),
    }));
    (
        visible.clone(),
        GatedWriter {
            state: state.clone(),
            visible: visible.clone(),
        },
        GateHandle { state, visible },
    )
}
