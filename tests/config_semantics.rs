use qsl_server::{
    app, AppState, Limits, ResourceControls, MAX_BODY_BYTES_CEILING, MAX_PUSH_RATE_BURST_CEILING,
    MAX_PUSH_RATE_REFILL_PER_SEC_CEILING, MAX_QUEUE_DEPTH_CEILING, MAX_ROUTE_COUNT_CEILING,
};
use reqwest::StatusCode as ReqStatus;
use serde::Deserialize;
use std::{
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use tokio::net::TcpListener;

const ROUTE_TOKEN_HEADER: &str = "X-QSL-Route-Token";
const AUTH_SENTINEL: &str = "NA0276_RELAY_TOKEN_SENTINEL_MUST_NOT_LEAK";

#[derive(Deserialize)]
struct PullItem {
    data: Vec<u8>,
}

#[derive(Deserialize)]
struct PullResp {
    items: Vec<PullItem>,
}

fn server_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_qsl-server"));
    command.env_clear();
    command.env("RUST_LOG", "error");
    command
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_config_failure(envs: &[(&str, &str)], expected_code: &str) {
    let mut command = server_command();
    command.env("RUST_BACKTRACE", "1");
    command.env("RELAY_TOKEN", AUTH_SENTINEL);
    for (name, value) in envs {
        command.env(name, value);
    }
    let output = command.output().unwrap_or_else(|e| panic!("{e}"));
    assert!(!output.status.success());
    let text = output_text(&output);
    assert!(
        text.contains(expected_code),
        "missing {expected_code}: {text}"
    );
    for forbidden in [
        AUTH_SENTINEL,
        "panicked at",
        "thread 'main' panicked",
        "stack backtrace",
    ] {
        assert!(
            !text.contains(forbidden),
            "failure output leaked {forbidden}"
        );
    }
}

fn assert_config_stays_running(envs: &[(&str, &str)]) {
    let mut command = server_command();
    command.env("PORT", "0");
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    for (name, value) in envs {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap_or_else(|e| panic!("{e}"));
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        if let Some(status) = child.try_wait().unwrap_or_else(|e| panic!("{e}")) {
            panic!("config expected to start but exited with {status}");
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait();
}

async fn spawn_server_with_auth(
    limits: Limits,
    relay_token: Option<&str>,
) -> (String, tokio::task::JoinHandle<()>) {
    let state = AppState::new_with_auth(limits, relay_token.map(str::to_string));
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

async fn push(
    client: &reqwest::Client,
    base: &str,
    route_token: &str,
    auth_token: Option<&str>,
    body: impl Into<Vec<u8>>,
) -> reqwest::Response {
    let mut request = client
        .post(format!("{base}/v1/push"))
        .header(ROUTE_TOKEN_HEADER, route_token)
        .body(body.into());
    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    request.send().await.unwrap_or_else(|e| panic!("{e}"))
}

async fn pull(
    client: &reqwest::Client,
    base: &str,
    route_token: &str,
    auth_token: Option<&str>,
    max: usize,
) -> reqwest::Response {
    let mut request = client
        .get(format!("{base}/v1/pull?max={max}"))
        .header(ROUTE_TOKEN_HEADER, route_token);
    if let Some(token) = auth_token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    request.send().await.unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn missing_size_depth_config_uses_safe_defaults() {
    assert_config_stays_running(&[]);
    let defaults = Limits::default();
    assert_eq!(defaults.max_body_bytes, MAX_BODY_BYTES_CEILING);
    assert_eq!(defaults.max_queue_depth, MAX_QUEUE_DEPTH_CEILING);
    let controls = ResourceControls::default();
    assert_eq!(controls.max_route_count, MAX_ROUTE_COUNT_CEILING);
    assert_eq!(controls.push_rate_burst, MAX_PUSH_RATE_BURST_CEILING);
    assert_eq!(
        controls.push_rate_refill_per_sec,
        MAX_PUSH_RATE_BURST_CEILING
    );
}

#[test]
fn malformed_or_zero_size_depth_config_fails_closed() {
    assert_config_failure(
        &[("PORT", "0"), ("MAX_BODY_BYTES", "not-a-size")],
        "ERR_INVALID_CONFIG_MAX_BODY_BYTES",
    );
    assert_config_failure(
        &[("PORT", "0"), ("MAX_BODY_BYTES", "0")],
        "ERR_INVALID_CONFIG_MAX_BODY_BYTES",
    );
    assert_config_failure(
        &[("PORT", "0"), ("MAX_QUEUE_DEPTH", "not-a-depth")],
        "ERR_INVALID_CONFIG_MAX_QUEUE_DEPTH",
    );
    assert_config_failure(
        &[("PORT", "0"), ("MAX_QUEUE_DEPTH", "0")],
        "ERR_INVALID_CONFIG_MAX_QUEUE_DEPTH",
    );
    assert_config_failure(
        &[("PORT", "0"), ("MAX_ROUTE_COUNT", "not-a-route-count")],
        "ERR_INVALID_CONFIG_MAX_ROUTE_COUNT",
    );
    assert_config_failure(
        &[("PORT", "0"), ("MAX_ROUTE_COUNT", "0")],
        "ERR_INVALID_CONFIG_MAX_ROUTE_COUNT",
    );
    assert_config_failure(
        &[("PORT", "0"), ("PUSH_RATE_BURST", "not-a-burst")],
        "ERR_INVALID_CONFIG_PUSH_RATE_BURST",
    );
    assert_config_failure(
        &[("PORT", "0"), ("PUSH_RATE_BURST", "0")],
        "ERR_INVALID_CONFIG_PUSH_RATE_BURST",
    );
    assert_config_failure(
        &[("PORT", "0"), ("PUSH_RATE_REFILL_PER_SEC", "not-a-refill")],
        "ERR_INVALID_CONFIG_PUSH_RATE_REFILL_PER_SEC",
    );
    assert_config_stays_running(&[("PUSH_RATE_REFILL_PER_SEC", "0")]);
}

#[test]
fn above_ceiling_size_depth_config_is_capped_explicitly() {
    let body_above_ceiling = (MAX_BODY_BYTES_CEILING + 1).to_string();
    let depth_above_ceiling = (MAX_QUEUE_DEPTH_CEILING + 1).to_string();
    let route_above_ceiling = (MAX_ROUTE_COUNT_CEILING + 1).to_string();
    let burst_above_ceiling = (MAX_PUSH_RATE_BURST_CEILING + 1).to_string();
    let refill_above_ceiling = (MAX_PUSH_RATE_REFILL_PER_SEC_CEILING + 1).to_string();
    assert_config_stays_running(&[
        ("MAX_BODY_BYTES", body_above_ceiling.as_str()),
        ("MAX_QUEUE_DEPTH", depth_above_ceiling.as_str()),
        ("MAX_ROUTE_COUNT", route_above_ceiling.as_str()),
        ("PUSH_RATE_BURST", burst_above_ceiling.as_str()),
        ("PUSH_RATE_REFILL_PER_SEC", refill_above_ceiling.as_str()),
    ]);
    let limits = Limits::new(MAX_BODY_BYTES_CEILING + 1, MAX_QUEUE_DEPTH_CEILING + 1).unwrap();
    let controls = ResourceControls::new(
        MAX_ROUTE_COUNT_CEILING + 1,
        MAX_PUSH_RATE_BURST_CEILING + 1,
        MAX_PUSH_RATE_REFILL_PER_SEC_CEILING + 1,
    )
    .unwrap();
    assert_eq!(limits.max_body_bytes, MAX_BODY_BYTES_CEILING);
    assert_eq!(limits.max_queue_depth, MAX_QUEUE_DEPTH_CEILING);
    assert_eq!(controls.max_route_count, MAX_ROUTE_COUNT_CEILING);
    assert_eq!(controls.push_rate_burst, MAX_PUSH_RATE_BURST_CEILING);
    assert_eq!(
        controls.push_rate_refill_per_sec,
        MAX_PUSH_RATE_REFILL_PER_SEC_CEILING
    );
}

#[test]
fn invalid_port_and_bind_addr_fail_closed() {
    assert_config_failure(&[("PORT", "not-a-port")], "ERR_INVALID_ENV_PORT");
    assert_config_failure(
        &[("PORT", "0"), ("BIND_ADDR", "not a socket address")],
        "ERR_BIND_PARSE",
    );
}

#[tokio::test(flavor = "current_thread")]
async fn effective_body_limit_rejects_above_selected_limit() {
    let (base, handle) = spawn_server_with_auth(Limits::new(4, 4).unwrap(), None).await;
    let client = reqwest::Client::new();

    let accepted = push(&client, &base, "body-route", None, b"fits".to_vec()).await;
    assert_eq!(accepted.status(), ReqStatus::OK);

    let rejected = push(&client, &base, "body-route", None, b"overs".to_vec()).await;
    assert_eq!(rejected.status(), ReqStatus::PAYLOAD_TOO_LARGE);
    assert_eq!(
        rejected.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_TOO_LARGE"
    );

    let delivered = pull(&client, &base, "body-route", None, 4).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].data.as_slice(), b"fits");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn effective_queue_depth_rejects_above_selected_limit() {
    let (base, handle) = spawn_server_with_auth(Limits::new(1024, 1).unwrap(), None).await;
    let client = reqwest::Client::new();

    let first = push(&client, &base, "depth-route", None, b"first".to_vec()).await;
    assert_eq!(first.status(), ReqStatus::OK);

    let second = push(&client, &base, "depth-route", None, b"second".to_vec()).await;
    assert_eq!(second.status(), ReqStatus::TOO_MANY_REQUESTS);
    assert_eq!(
        second.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_OVERLOADED"
    );

    let delivered = pull(&client, &base, "depth-route", None, 2).await;
    assert_eq!(delivered.status(), ReqStatus::OK);
    let body: PullResp = delivered.json().await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(body.items.len(), 1);
    assert_eq!(body.items[0].data.as_slice(), b"first");

    handle.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn relay_token_missing_and_present_modes_are_explicit() {
    let (open_base, open_handle) =
        spawn_server_with_auth(Limits::new(1024, 4).unwrap(), None).await;
    let client = reqwest::Client::new();
    let open = push(&client, &open_base, "open-route", None, b"open".to_vec()).await;
    assert_eq!(open.status(), ReqStatus::OK);
    open_handle.abort();

    let (auth_base, auth_handle) = spawn_server_with_auth(
        Limits::new(1024, 4).unwrap(),
        Some("NA0276_REQUIRED_AUTH_TOKEN"),
    )
    .await;
    let missing = push(&client, &auth_base, "auth-route", None, b"missing".to_vec()).await;
    assert_eq!(missing.status(), ReqStatus::UNAUTHORIZED);
    assert_eq!(
        missing.text().await.unwrap_or_else(|e| panic!("{e}")),
        "ERR_UNAUTHORIZED"
    );

    let accepted = push(
        &client,
        &auth_base,
        "auth-route",
        Some("NA0276_REQUIRED_AUTH_TOKEN"),
        b"accepted".to_vec(),
    )
    .await;
    assert_eq!(accepted.status(), ReqStatus::OK);
    let delivered = pull(
        &client,
        &auth_base,
        "auth-route",
        Some("NA0276_REQUIRED_AUTH_TOKEN"),
        1,
    )
    .await;
    assert_eq!(delivered.status(), ReqStatus::OK);

    auth_handle.abort();
}
