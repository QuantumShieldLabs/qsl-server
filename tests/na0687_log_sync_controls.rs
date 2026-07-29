//! NA-0687 / D621 §8 — the RED-CAPABLE CONTROLS for the log-capture synchronisation
//! helper. **A fix whose control cannot fire has fixed nothing.**
//!
//! These tests exercise the INSTRUMENT (`tests/common/mod.rs`), not the relay. They
//! are deterministic: the gated writer makes the lost race an explicit state rather
//! than a scheduler accident, so nothing here depends on core count, thread count or
//! luck.
//!
//! The four permanent controls, and what each one would catch:
//!
//! * **A′** — the gate really withholds. Without this, control A could go green and
//!   "prove" the defect cannot happen. ⚠ The control instrument needs a control; that
//!   is ENG-0089's lesson one surface over.
//! * **B** — a line that arrives late is still observed. This is the fix working.
//! * **C** — a line that never arrives produces the NAMED timeout over an **empty**
//!   buffer, so the wait is neither vacuous nor unbounded.
//! * **C2** — a line that never arrives produces a timeout that reports a **populated**
//!   buffer distinctly. *Nothing emitted at all* and *the wrong thing emitted* are
//!   different defects and must not be reported by the same words.
//!
//! Control **A** — the UNFIXED shape (read immediately, assert) under a withheld gate,
//! which must go **RED** — cannot live in a green suite. It was applied temporarily,
//! captured, and reverted, with the revert proved; see the lane's evidence.

mod common;

use common::{await_log, capture, gated_capture, log_text, try_await_log, LogWaitError};
use std::time::{Duration, Instant};
use tracing::subscriber::set_default;

/// The needle every control uses: the same redacted-id token 7 of the 12 swept sites
/// already assert positively. Vocabulary adopted, not invented.
const NEEDLE: &str = "channel_id=";

#[tokio::test(flavor = "current_thread")]
async fn control_a_prime_the_gate_withholds_until_released() {
    let (buf, w, gate) = gated_capture();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || w.clone())
        .finish();
    let _guard = set_default(subscriber);

    tracing::info!("{NEEDLE}deadbeef");

    // The line HAS been emitted. A reader that lost the race sees nothing -- which is
    // precisely what makes control A's red deterministic rather than lucky.
    assert!(
        !log_text(&buf).contains(NEEDLE),
        "the gate must withhold what was written before release, or control A proves nothing"
    );

    gate.release();

    assert!(
        log_text(&buf).contains(NEEDLE),
        "release must reveal everything staged while the gate was closed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn control_b_await_log_observes_a_line_that_arrives_late() {
    let (buf, w, gate) = gated_capture();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || w.clone())
        .finish();
    let _guard = set_default(subscriber);

    tracing::info!("{NEEDLE}deadbeef");

    // Release from another task, inside the deadline: the emit is already done, only
    // its VISIBILITY is delayed -- the shape of the real defect.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        gate.release();
    });

    let start = Instant::now();
    let text = await_log(&buf, NEEDLE).await;
    let waited = start.elapsed();

    assert!(
        text.contains(NEEDLE),
        "the awaited line must be in the text returned"
    );
    // Proves the helper WAITED rather than happening to read a populated buffer.
    assert!(
        waited >= Duration::from_millis(150),
        "await_log returned in {waited:?}, before the line could have become visible"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn control_c_never_released_reports_the_named_timeout_over_an_empty_buffer() {
    let (buf, w, _gate) = gated_capture();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || w.clone())
        .finish();
    let _guard = set_default(subscriber);

    tracing::info!("{NEEDLE}deadbeef");

    // The gate is never released. The wait must END -- bounded, not hung -- and must
    // say what it examined.
    let err = match try_await_log(&buf, NEEDLE).await {
        Ok(text) => panic!("the wait must NOT succeed while the gate is closed; got {text:?}"),
        Err(e) => e,
    };

    let LogWaitError::Timeout {
        needle,
        waited_ms,
        bytes,
        lines,
    } = &err;
    assert_eq!(needle, NEEDLE);
    assert!(
        *waited_ms >= 5_000,
        "waited {waited_ms}ms, expected the full 5 s deadline"
    );
    assert_eq!(
        *bytes, 0,
        "an unreleased gate leaves the visible buffer EMPTY"
    );
    assert_eq!(*lines, 0);
    assert!(
        err.to_string().contains("LOG_SYNC_TIMEOUT"),
        "the failure must be NAMED, not anonymous: {err}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn control_c2_a_populated_buffer_times_out_with_its_size_reported() {
    let (buf, w) = capture();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || w.clone())
        .finish();
    let _guard = set_default(subscriber);

    // Something WAS logged -- just not the needle.
    tracing::info!("event=something_else");

    let err = match try_await_log(&buf, NEEDLE).await {
        Ok(text) => panic!("the needle was never emitted; got {text:?}"),
        Err(e) => e,
    };

    let LogWaitError::Timeout { bytes, lines, .. } = &err;
    // ⚠ THIS is the distinction that stops a vacuous pass being indistinguishable
    // from a real one: the timeout reports a POPULATED buffer, so "nothing emitted"
    // and "the wrong thing emitted" are different diagnoses at a glance.
    assert!(
        *bytes > 0,
        "the buffer was written to; the error must say so"
    );
    assert!(*lines >= 1);
    assert!(err.to_string().contains("LOG_SYNC_TIMEOUT"));
}
