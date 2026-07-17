use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::info;
use uuid::Uuid;

mod store;

pub use store::{
    pull_lease_or_error, retention_ttl_or_error, StoreConfig, SweepStats, MAX_ACK_IDS,
    MAX_PULL_LEASE_SECS_CEILING, MAX_RETENTION_TTL_SECS_CEILING, PULL_LEASE_SECS_DEFAULT,
    RETENTION_TTL_SECS_DEFAULT,
};

use store::{now_unix_secs, AckOutcome, EnqueueOutcome, PullMode, PullOutcome, Store};

type RateBuckets = HashMap<String, PushRateBucket>;

#[derive(Debug)]
struct PushRateBucket {
    tokens: usize,
    last_refill: Instant,
}

impl PushRateBucket {
    fn new(now: Instant, controls: ResourceControls) -> Self {
        Self {
            tokens: controls.push_rate_burst,
            last_refill: now,
        }
    }

    fn try_consume(&mut self, controls: ResourceControls, now: Instant) -> bool {
        self.refill(controls, now);
        if self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }

    fn refill(&mut self, controls: ResourceControls, now: Instant) {
        if controls.push_rate_refill_per_sec == 0 || self.tokens >= controls.push_rate_burst {
            return;
        }
        let elapsed_secs = now
            .checked_duration_since(self.last_refill)
            .unwrap_or(Duration::ZERO)
            .as_secs() as usize;
        if elapsed_secs == 0 {
            return;
        }
        let refill = elapsed_secs.saturating_mul(controls.push_rate_refill_per_sec);
        self.tokens = self
            .tokens
            .saturating_add(refill)
            .min(controls.push_rate_burst);
        self.last_refill = now;
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_body_bytes: usize,
    pub max_queue_depth: usize,
}

pub const MAX_BODY_BYTES_CEILING: usize = 1024 * 1024;
pub const MAX_QUEUE_DEPTH_CEILING: usize = 257;
pub const MAX_ROUTE_COUNT_CEILING: usize = 256;
pub const MAX_PUSH_RATE_BURST_CEILING: usize = 257;
pub const MAX_PUSH_RATE_REFILL_PER_SEC_CEILING: usize = 4096;
pub const ROUTE_IDLE_TTL_MS_DEFAULT: usize = 300_000;
pub const MAX_ROUTE_IDLE_TTL_MS_CEILING: usize = 86_400_000;

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_body_bytes: MAX_BODY_BYTES_CEILING,
            max_queue_depth: MAX_QUEUE_DEPTH_CEILING,
        }
    }
}

impl Limits {
    pub fn new(max_body_bytes: usize, max_queue_depth: usize) -> Result<Self, String> {
        Ok(Self {
            max_body_bytes: limit_or_error(
                "MAX_BODY_BYTES",
                max_body_bytes,
                MAX_BODY_BYTES_CEILING,
            )?,
            max_queue_depth: limit_or_error(
                "MAX_QUEUE_DEPTH",
                max_queue_depth,
                MAX_QUEUE_DEPTH_CEILING,
            )?,
        })
    }

    pub fn from_env() -> Result<Self, String> {
        let max_body_bytes = env_limit_or_default("MAX_BODY_BYTES", MAX_BODY_BYTES_CEILING)?;
        let max_queue_depth = env_limit_or_default("MAX_QUEUE_DEPTH", MAX_QUEUE_DEPTH_CEILING)?;
        Self::new(max_body_bytes, max_queue_depth)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ResourceControls {
    pub max_route_count: usize,
    pub push_rate_burst: usize,
    pub push_rate_refill_per_sec: usize,
    // Vestigial since NA-0642: message lifetime is now governed by the store's
    // retention TTL (StoreConfig::retention_ttl_secs); kept so existing
    // constructor call sites remain source-compatible.
    pub route_idle_ttl: Duration,
}

impl Default for ResourceControls {
    fn default() -> Self {
        Self {
            max_route_count: MAX_ROUTE_COUNT_CEILING,
            push_rate_burst: MAX_PUSH_RATE_BURST_CEILING,
            push_rate_refill_per_sec: MAX_PUSH_RATE_BURST_CEILING,
            route_idle_ttl: Duration::from_millis(ROUTE_IDLE_TTL_MS_DEFAULT as u64),
        }
    }
}

impl ResourceControls {
    pub fn new(
        max_route_count: usize,
        push_rate_burst: usize,
        push_rate_refill_per_sec: usize,
    ) -> Result<Self, String> {
        Self::new_with_route_idle_ttl_ms(
            max_route_count,
            push_rate_burst,
            push_rate_refill_per_sec,
            ROUTE_IDLE_TTL_MS_DEFAULT,
        )
    }

    pub fn new_with_route_idle_ttl_ms(
        max_route_count: usize,
        push_rate_burst: usize,
        push_rate_refill_per_sec: usize,
        route_idle_ttl_ms: usize,
    ) -> Result<Self, String> {
        Ok(Self {
            max_route_count: limit_or_error(
                "MAX_ROUTE_COUNT",
                max_route_count,
                MAX_ROUTE_COUNT_CEILING,
            )?,
            push_rate_burst: limit_or_error(
                "PUSH_RATE_BURST",
                push_rate_burst,
                MAX_PUSH_RATE_BURST_CEILING,
            )?,
            push_rate_refill_per_sec: push_rate_refill_or_cap(push_rate_refill_per_sec),
            route_idle_ttl: Duration::from_millis(limit_or_error(
                "ROUTE_IDLE_TTL_MS",
                route_idle_ttl_ms,
                MAX_ROUTE_IDLE_TTL_MS_CEILING,
            )? as u64),
        })
    }

    pub fn from_env() -> Result<Self, String> {
        let max_route_count = env_limit_or_default("MAX_ROUTE_COUNT", MAX_ROUTE_COUNT_CEILING)?;
        let push_rate_burst = env_limit_or_default("PUSH_RATE_BURST", MAX_PUSH_RATE_BURST_CEILING)?;
        let push_rate_refill_per_sec =
            env_refill_or_default("PUSH_RATE_REFILL_PER_SEC", MAX_PUSH_RATE_BURST_CEILING)?;
        Self::new(max_route_count, push_rate_burst, push_rate_refill_per_sec)
    }
}

fn limit_or_error(name: &str, value: usize, ceiling: usize) -> Result<usize, String> {
    if value == 0 {
        return Err(format!("ERR_INVALID_CONFIG_{name}"));
    }
    Ok(value.min(ceiling))
}

fn env_limit_or_default(name: &str, default: usize) -> Result<usize, String> {
    match std::env::var(name).ok().filter(|v| !v.trim().is_empty()) {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| format!("ERR_INVALID_CONFIG_{name}")),
        None => Ok(default),
    }
}

fn env_refill_or_default(name: &str, default: usize) -> Result<usize, String> {
    match std::env::var(name).ok().filter(|v| !v.trim().is_empty()) {
        Some(value) => value
            .parse::<usize>()
            .map(push_rate_refill_or_cap)
            .map_err(|_| format!("ERR_INVALID_CONFIG_{name}")),
        None => Ok(default),
    }
}

fn push_rate_refill_or_cap(value: usize) -> usize {
    value.min(MAX_PUSH_RATE_REFILL_PER_SEC_CEILING)
}

// NA-0652 server-info descriptive fields. Reporting-only: nothing here gates
// any relay behavior (the capability document advertises FEATURES, never
// SECURITY — DOC-SRV-006). All three are optional; absent or empty env vars
// never fail startup, matching the RELAY_TOKEN env-only precedent.
#[derive(Clone, Debug, Default)]
pub struct ServerInfoCfg {
    pub name: Option<String>,
    pub attachments_service_url: Option<String>,
    pub min_client_version: Option<String>,
}

impl ServerInfoCfg {
    pub fn from_env() -> Self {
        Self {
            name: std::env::var("RELAY_NAME")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            attachments_service_url: std::env::var("RELAY_ATTACHMENTS_SERVICE_URL")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            min_client_version: std::env::var("RELAY_MIN_CLIENT_VERSION")
                .ok()
                .filter(|v| !v.trim().is_empty()),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    // Durable store-and-forward queue (SQLite). Payloads are opaque blobs;
    // route tokens are stored only as SHA-256 digests.
    store: Store,
    // Per-route push rate accounting stays in memory: abuse control, not
    // correctness state; resets on restart by design.
    push_rates: Arc<Mutex<RateBuckets>>,
    limits: Limits,
    controls: ResourceControls,
    relay_token: Option<String>,
    // Validated copy of StoreConfig::retention_ttl_secs so /v1/server-info can
    // report the live value the store enforces (same retention_ttl_or_error
    // path Store::open applies).
    retention_ttl_secs: usize,
    server_info: ServerInfoCfg,
}

impl AppState {
    pub fn new(limits: Limits) -> Self {
        Self::new_with_auth(
            limits,
            std::env::var("RELAY_TOKEN").ok().filter(|v| !v.is_empty()),
        )
    }

    pub fn new_with_auth(limits: Limits, relay_token: Option<String>) -> Self {
        Self::new_with_auth_and_controls(limits, ResourceControls::default(), relay_token)
    }

    pub fn new_with_controls(limits: Limits, controls: ResourceControls) -> Self {
        Self::new_with_auth_and_controls(
            limits,
            controls,
            std::env::var("RELAY_TOKEN").ok().filter(|v| !v.is_empty()),
        )
    }

    pub fn new_with_auth_and_controls(
        limits: Limits,
        controls: ResourceControls,
        relay_token: Option<String>,
    ) -> Self {
        Self::new_with_auth_controls_and_store(
            limits,
            controls,
            relay_token,
            StoreConfig::default(),
        )
        .expect("ERR_STORE_OPEN_IN_MEMORY")
    }

    pub fn new_with_controls_and_store(
        limits: Limits,
        controls: ResourceControls,
        store_cfg: StoreConfig,
    ) -> Result<Self, String> {
        Self::new_with_auth_controls_and_store(
            limits,
            controls,
            std::env::var("RELAY_TOKEN").ok().filter(|v| !v.is_empty()),
            store_cfg,
        )
    }

    pub fn new_with_auth_controls_and_store(
        limits: Limits,
        controls: ResourceControls,
        relay_token: Option<String>,
        store_cfg: StoreConfig,
    ) -> Result<Self, String> {
        Self::new_with_auth_controls_store_and_info(
            limits,
            controls,
            relay_token,
            store_cfg,
            ServerInfoCfg::from_env(),
        )
    }

    pub fn new_with_auth_controls_store_and_info(
        limits: Limits,
        controls: ResourceControls,
        relay_token: Option<String>,
        store_cfg: StoreConfig,
        server_info: ServerInfoCfg,
    ) -> Result<Self, String> {
        let retention_ttl_secs = retention_ttl_or_error(store_cfg.retention_ttl_secs)?;
        Ok(Self {
            store: Store::open(&store_cfg)?,
            push_rates: Arc::new(Mutex::new(HashMap::new())),
            limits,
            controls,
            relay_token,
            retention_ttl_secs,
            server_info,
        })
    }

    /// Delete undelivered messages older than the retention TTL. Runs lazily
    /// on every push/pull as well; this entry point exists for the periodic
    /// background sweep so quiet relays still expire.
    pub fn run_retention_sweep(&self) -> SweepStats {
        let now = now_unix_secs();
        match self.store.retention_sweep(now) {
            Ok(stats) => {
                self.log_and_prune_sweep(&stats);
                stats
            }
            Err(e) => {
                tracing::error!("event=store_error op=retention_sweep detail={e}");
                SweepStats::default()
            }
        }
    }

    fn log_and_prune_sweep(&self, stats: &SweepStats) {
        for (log_id, expired) in &stats.expired_routes {
            info!(
                "event=retention_expired channel_id={} expired_messages={} ttl_secs={}",
                log_id,
                expired,
                self.store.retention_ttl_secs()
            );
        }
        if !stats.removed_route_keys.is_empty() {
            if let Ok(mut buckets) = self.push_rates.lock() {
                for key in &stats.removed_route_keys {
                    buckets.remove(key);
                }
            }
        }
    }

    fn drop_rate_bucket(&self, route_key: &str) {
        if let Ok(mut buckets) = self.push_rates.lock() {
            buckets.remove(route_key);
        }
    }
}

#[derive(Serialize)]
struct PostResp {
    id: String,
}

#[derive(Serialize)]
struct PullItem {
    id: String,
    data: Vec<u8>,
}

#[derive(Serialize)]
struct PullResp {
    items: Vec<PullItem>,
}

#[derive(serde::Deserialize)]
struct PullQuery {
    max: Option<usize>,
    ack: Option<String>,
}

#[derive(serde::Deserialize)]
struct AckReq {
    ids: Vec<String>,
}

#[derive(Serialize)]
struct AckResp {
    acked: usize,
}

const ROUTE_TOKEN_HEADER: &str = "x-qsl-route-token";

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/v1/push", post(push_message))
        .route("/v1/pull", get(pull_message))
        .route("/v1/pull/ack", post(ack_messages))
        .route("/v1/server-info", get(server_info))
        .with_state(state)
}

// NA-0652 capability document (DOC-SRV-006). Served behind the same bearer
// gate as the relay routes; on a bearer relay an unauthorized request gets the
// FIXED two-key probe — identical for a missing and a wrong token, so the
// response is never a token oracle, and the registry never grows operator
// config. The full document reports LIVE config only; it gates features,
// never security.
async fn server_info(State(st): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if !auth_ok(&headers, st.relay_token.as_deref()) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "server": "qsl-server",
                "auth": { "mode": "bearer" }
            })),
        )
            .into_response();
    }
    let auth_mode = if st.relay_token.is_some() {
        "bearer"
    } else {
        "open"
    };
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "server": "qsl-server",
            "version": env!("CARGO_PKG_VERSION"),
            "name": st.server_info.name.clone().unwrap_or_default(),
            "api": ["push_v1", "pull_v1", "pull_ack_lease_v1"],
            "auth": { "mode": auth_mode },
            "limits": {
                "max_body_bytes": st.limits.max_body_bytes,
                "max_queue_depth": st.limits.max_queue_depth,
            },
            "retention": { "ttl_secs": st.retention_ttl_secs },
            "directory": { "mode": "none" },
            "attachments": { "service_url": st.server_info.attachments_service_url },
            "kt": { "mode": "none" },
            "min_client_version": st.server_info.min_client_version,
        })),
    )
        .into_response()
}

fn channel_log_id(channel: &str) -> String {
    // Deterministic redaction-safe identifier (FNV-1a 64-bit rendered as hex).
    let mut state: u64 = 0xcbf29ce484222325;
    for b in channel.as_bytes() {
        state ^= u64::from(*b);
        state = state.wrapping_mul(0x100000001b3);
    }
    format!("{state:016x}")
}

fn route_key_for(channel: &str) -> String {
    // At-rest key: the raw route token is never persisted; lookups hash the
    // header value, so a stolen store file yields no usable routing tokens.
    let digest = Sha256::digest(channel.as_bytes());
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn auth_ok(headers: &HeaderMap, relay_token: Option<&str>) -> bool {
    match relay_token {
        None => true,
        Some(token) => {
            let Some(raw) = headers.get("authorization").and_then(|v| v.to_str().ok()) else {
                return false;
            };
            let Some(provided) = raw.strip_prefix("Bearer ") else {
                return false;
            };
            provided == token
        }
    }
}

fn resolve_route_token(headers: &HeaderMap) -> Result<String, &'static str> {
    match headers.get(ROUTE_TOKEN_HEADER) {
        None => Err("ERR_MISSING_ROUTE_TOKEN"),
        Some(raw) => {
            let raw = raw.to_str().map_err(|_| "ERR_BAD_ROUTE_TOKEN")?;
            let token = raw.trim();
            if token.is_empty() {
                return Err("ERR_MISSING_ROUTE_TOKEN");
            }
            Ok(token.to_string())
        }
    }
}

async fn run_store<T, F>(f: F) -> Result<T, &'static str>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => {
            tracing::error!("event=store_error detail={e}");
            Err("ERR_STORE")
        }
        Err(_) => Err("ERR_STORE"),
    }
}

async fn push_message(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if !auth_ok(&headers, st.relay_token.as_deref()) {
        return (StatusCode::UNAUTHORIZED, "ERR_UNAUTHORIZED").into_response();
    }
    let channel = match resolve_route_token(&headers) {
        Ok(v) => v,
        Err(code) => return (StatusCode::BAD_REQUEST, code).into_response(),
    };
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, "ERR_EMPTY_BODY").into_response();
    }
    if body.len() > st.limits.max_body_bytes {
        return (StatusCode::PAYLOAD_TOO_LARGE, "ERR_TOO_LARGE").into_response();
    }

    let msg_id = headers
        .get("x-msg-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let route_key = route_key_for(&channel);
    let log_id = channel_log_id(&channel);
    let now = now_unix_secs();

    let status = {
        let store = st.store.clone();
        let key = route_key.clone();
        match run_store(move || store.route_status(&key, now)).await {
            Ok(v) => v,
            Err(code) => return (StatusCode::INTERNAL_SERVER_ERROR, code).into_response(),
        }
    };
    st.log_and_prune_sweep(&status.sweep);

    if status.route_exists && status.depth >= st.limits.max_queue_depth {
        info!(
            "event=overloaded queue_depth={} max={}",
            status.depth, st.limits.max_queue_depth
        );
        return (StatusCode::TOO_MANY_REQUESTS, "ERR_OVERLOADED").into_response();
    }
    if !status.route_exists && status.live_routes >= st.controls.max_route_count {
        info!(
            "event=route_cap channel_id={} live_routes={} max={}",
            log_id, status.live_routes, st.controls.max_route_count
        );
        return (StatusCode::TOO_MANY_REQUESTS, "ERR_ROUTE_CAP").into_response();
    }

    {
        let mono_now = Instant::now();
        let Ok(mut buckets) = st.push_rates.lock() else {
            return (StatusCode::INTERNAL_SERVER_ERROR, "ERR_LOCK_POISON").into_response();
        };
        let bucket = buckets
            .entry(route_key.clone())
            .or_insert_with(|| PushRateBucket::new(mono_now, st.controls));
        if !bucket.try_consume(st.controls, mono_now) {
            info!(
                "event=rate_limited channel_id={} burst={} refill_per_sec={}",
                log_id, st.controls.push_rate_burst, st.controls.push_rate_refill_per_sec
            );
            return (StatusCode::TOO_MANY_REQUESTS, "ERR_RATE_LIMITED").into_response();
        }
    }

    let outcome = {
        let store = st.store.clone();
        let key = route_key.clone();
        let lid = log_id.clone();
        let mid = msg_id.clone();
        let payload = body.to_vec();
        let max_depth = st.limits.max_queue_depth;
        let max_routes = st.controls.max_route_count;
        match run_store(move || {
            store.enqueue(&key, &lid, &mid, &payload, now, max_depth, max_routes)
        })
        .await
        {
            Ok(v) => v,
            Err(code) => return (StatusCode::INTERNAL_SERVER_ERROR, code).into_response(),
        }
    };
    match outcome {
        EnqueueOutcome::Accepted => {}
        EnqueueOutcome::Overloaded { depth } => {
            info!(
                "event=overloaded queue_depth={} max={}",
                depth, st.limits.max_queue_depth
            );
            return (StatusCode::TOO_MANY_REQUESTS, "ERR_OVERLOADED").into_response();
        }
        EnqueueOutcome::RouteCap { live_routes } => {
            info!(
                "event=route_cap channel_id={} live_routes={} max={}",
                log_id, live_routes, st.controls.max_route_count
            );
            st.drop_rate_bucket(&route_key);
            return (StatusCode::TOO_MANY_REQUESTS, "ERR_ROUTE_CAP").into_response();
        }
    }

    // Never log payload; metadata only.
    info!(
        "push channel_id={} id={} bytes={}",
        log_id,
        msg_id,
        body.len()
    );

    (StatusCode::OK, Json(PostResp { id: msg_id })).into_response()
}

async fn pull_message(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PullQuery>,
) -> impl IntoResponse {
    if !auth_ok(&headers, st.relay_token.as_deref()) {
        return (StatusCode::UNAUTHORIZED, "ERR_UNAUTHORIZED").into_response();
    }
    let channel = match resolve_route_token(&headers) {
        Ok(v) => v,
        Err(code) => return (StatusCode::BAD_REQUEST, code).into_response(),
    };
    let max = query.max.unwrap_or(1);
    if max == 0 {
        return (StatusCode::BAD_REQUEST, "ERR_BAD_MAX").into_response();
    }
    let max = max.min(st.limits.max_queue_depth);
    let mode = match query.ack.as_deref() {
        None => PullMode::Legacy,
        Some("lease") => PullMode::Lease,
        Some(_) => return (StatusCode::BAD_REQUEST, "ERR_BAD_ACK_MODE").into_response(),
    };
    let route_key = route_key_for(&channel);
    let log_id = channel_log_id(&channel);
    let now = now_unix_secs();

    let outcome: PullOutcome = {
        let store = st.store.clone();
        let key = route_key.clone();
        match run_store(move || store.pull(&key, max, now, mode)).await {
            Ok(v) => v,
            Err(code) => return (StatusCode::INTERNAL_SERVER_ERROR, code).into_response(),
        }
    };
    st.log_and_prune_sweep(&outcome.sweep);
    if outcome.route_drained {
        st.drop_rate_bucket(&route_key);
    }
    if outcome.items.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    let mut items = Vec::with_capacity(outcome.items.len());
    for msg in outcome.items {
        info!(
            "pull channel_id={} id={} bytes={}",
            log_id,
            msg.msg_id,
            msg.body.len()
        );
        items.push(PullItem {
            id: msg.msg_id,
            data: msg.body,
        });
    }
    (StatusCode::OK, Json(PullResp { items })).into_response()
}

async fn ack_messages(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    if !auth_ok(&headers, st.relay_token.as_deref()) {
        return (StatusCode::UNAUTHORIZED, "ERR_UNAUTHORIZED").into_response();
    }
    let channel = match resolve_route_token(&headers) {
        Ok(v) => v,
        Err(code) => return (StatusCode::BAD_REQUEST, code).into_response(),
    };
    let Ok(req) = serde_json::from_slice::<AckReq>(&body) else {
        return (StatusCode::BAD_REQUEST, "ERR_BAD_ACK_BODY").into_response();
    };
    if req.ids.is_empty() || req.ids.len() > MAX_ACK_IDS {
        return (StatusCode::BAD_REQUEST, "ERR_BAD_ACK_IDS").into_response();
    }
    let route_key = route_key_for(&channel);
    let log_id = channel_log_id(&channel);
    let now = now_unix_secs();

    let outcome: AckOutcome = {
        let store = st.store.clone();
        let key = route_key.clone();
        let ids = req.ids;
        match run_store(move || store.ack(&key, &ids, now)).await {
            Ok(v) => v,
            Err(code) => return (StatusCode::INTERNAL_SERVER_ERROR, code).into_response(),
        }
    };
    st.log_and_prune_sweep(&outcome.sweep);
    if outcome.route_drained {
        st.drop_rate_bucket(&route_key);
    }
    info!("ack channel_id={} acked={}", log_id, outcome.acked);
    (
        StatusCode::OK,
        Json(AckResp {
            acked: outcome.acked,
        }),
    )
        .into_response()
}
#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode as ReqStatus;
    use serde::Deserialize;
    use tokio::net::TcpListener;
    use tracing::subscriber::set_default;

    #[derive(Clone)]
    struct SharedWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut g = self.0.lock().unwrap_or_else(|e| panic!("{e}"));
            g.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    async fn spawn_server_with_token(
        limits: Limits,
        relay_token: Option<String>,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let state = AppState::new_with_auth(limits, relay_token);
        let app = app(state);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let addr = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .unwrap_or_else(|e| panic!("{e}"));
        });
        (format!("http://{}", addr), handle)
    }

    async fn spawn_server(limits: Limits) -> (String, tokio::task::JoinHandle<()>) {
        spawn_server_with_token(limits, None).await
    }

    #[derive(Deserialize)]
    struct PullItem {
        id: String,
        data: Vec<u8>,
    }

    #[derive(Deserialize)]
    struct PullResp {
        items: Vec<PullItem>,
    }

    async fn canonical_push(
        client: &reqwest::Client,
        base: &str,
        token: &str,
        body: Vec<u8>,
    ) -> reqwest::Response {
        client
            .post(format!("{}/v1/push", base))
            .header(ROUTE_TOKEN_HEADER, token)
            .body(body)
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"))
    }

    async fn canonical_pull(
        client: &reqwest::Client,
        base: &str,
        token: &str,
        max: usize,
    ) -> reqwest::Response {
        client
            .get(format!("{}/v1/pull?max={}", base, max))
            .header(ROUTE_TOKEN_HEADER, token)
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"))
    }

    #[tokio::test]
    async fn push_then_pull_roundtrip() {
        let (base, handle) = spawn_server(Limits {
            max_body_bytes: 1024 * 1024,
            max_queue_depth: 8,
        })
        .await;

        let client = reqwest::Client::new();
        let payload = b"opaque-bytes".to_vec();
        let push = canonical_push(&client, &base, "test", payload.clone()).await;
        assert_eq!(push.status(), ReqStatus::OK);

        let pull = canonical_pull(&client, &base, "test", 1).await;
        assert_eq!(pull.status(), ReqStatus::OK);
        let body: PullResp = pull.json().await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body.items.len(), 1);
        assert!(!body.items[0].id.is_empty());
        assert_eq!(body.items[0].data.as_slice(), payload.as_slice());

        handle.abort();
    }

    #[tokio::test]
    async fn pull_empty_returns_204() {
        let (base, handle) = spawn_server(Limits {
            max_body_bytes: 1024 * 1024,
            max_queue_depth: 8,
        })
        .await;
        let client = reqwest::Client::new();
        let pull = canonical_pull(&client, &base, "empty", 1).await;
        assert_eq!(pull.status(), ReqStatus::NO_CONTENT);
        handle.abort();
    }

    #[tokio::test]
    async fn oversize_returns_413() {
        let (base, handle) = spawn_server(Limits {
            max_body_bytes: 4,
            max_queue_depth: 8,
        })
        .await;
        let client = reqwest::Client::new();
        let push = canonical_push(&client, &base, "oversize", vec![0u8; 5]).await;
        assert_eq!(push.status(), ReqStatus::PAYLOAD_TOO_LARGE);
        let body = push.text().await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body, "ERR_TOO_LARGE");

        let pull = canonical_pull(&client, &base, "oversize", 1).await;
        assert_eq!(pull.status(), ReqStatus::NO_CONTENT);
        handle.abort();
    }

    #[tokio::test]
    async fn queue_full_returns_429() {
        let (base, handle) = spawn_server(Limits {
            max_body_bytes: 1024 * 1024,
            max_queue_depth: 1,
        })
        .await;
        let client = reqwest::Client::new();
        let r1 = canonical_push(&client, &base, "qfull", b"a".to_vec()).await;
        assert_eq!(r1.status(), ReqStatus::OK);

        let r2 = canonical_push(&client, &base, "qfull", b"b".to_vec()).await;
        assert_eq!(r2.status(), ReqStatus::TOO_MANY_REQUESTS);
        let body = r2.text().await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body, "ERR_OVERLOADED");

        let pull1 = canonical_pull(&client, &base, "qfull", 1).await;
        assert_eq!(pull1.status(), ReqStatus::OK);
        let body1: PullResp = pull1.json().await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body1.items.len(), 1);
        assert!(!body1.items[0].id.is_empty());

        let pull2 = canonical_pull(&client, &base, "qfull", 1).await;
        assert_eq!(pull2.status(), ReqStatus::NO_CONTENT);
        handle.abort();
    }

    #[tokio::test]
    async fn pull_deletes_on_deliver() {
        let (base, handle) = spawn_server(Limits {
            max_body_bytes: 1024 * 1024,
            max_queue_depth: 8,
        })
        .await;
        let client = reqwest::Client::new();

        let _ = canonical_push(&client, &base, "two", b"a".to_vec()).await;
        let _ = canonical_push(&client, &base, "two", b"b".to_vec()).await;

        let pull1 = canonical_pull(&client, &base, "two", 1).await;
        assert_eq!(pull1.status(), ReqStatus::OK);
        let body1: PullResp = pull1.json().await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body1.items.len(), 1);

        let pull2 = canonical_pull(&client, &base, "two", 2).await;
        assert_eq!(pull2.status(), ReqStatus::OK);
        let body2: PullResp = pull2.json().await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body2.items.len(), 1);
        assert!(!body2.items[0].id.is_empty());

        let pull3 = canonical_pull(&client, &base, "two", 1).await;
        assert_eq!(pull3.status(), ReqStatus::NO_CONTENT);

        handle.abort();
    }

    #[tokio::test]
    async fn payload_not_logged() {
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let writer = SharedWriter(buf.clone());
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = set_default(subscriber);

        let (base, handle) = spawn_server(Limits {
            max_body_bytes: 1024 * 1024,
            max_queue_depth: 8,
        })
        .await;
        let client = reqwest::Client::new();
        let payload = b"SECRET_PAYLOAD_ABC".to_vec();
        let _ = canonical_push(&client, &base, "nolog", payload).await;

        handle.abort();

        let binding = buf.lock().unwrap_or_else(|e| panic!("{e}"));
        let logged = String::from_utf8_lossy(&binding);
        assert!(!logged.contains("SECRET_PAYLOAD_ABC"));
    }

    #[test]
    fn channel_log_id_is_deterministic_and_redacted() {
        let raw = "alice-route-token-123";
        let id1 = channel_log_id(raw);
        let id2 = channel_log_id(raw);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 16);
        assert_ne!(id1, raw);
    }

    #[tokio::test]
    async fn logs_do_not_contain_raw_channel() {
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let writer = SharedWriter(buf.clone());
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = set_default(subscriber);

        let channel = "route-token-sensitive";
        let (base, handle) = spawn_server(Limits {
            max_body_bytes: 1024 * 1024,
            max_queue_depth: 8,
        })
        .await;
        let client = reqwest::Client::new();
        let payload = b"log-redaction".to_vec();
        let push = canonical_push(&client, &base, channel, payload).await;
        assert_eq!(push.status(), ReqStatus::OK);
        let pull = canonical_pull(&client, &base, channel, 1).await;
        assert_eq!(pull.status(), ReqStatus::OK);

        handle.abort();

        let binding = buf.lock().unwrap_or_else(|e| panic!("{e}"));
        let logged = String::from_utf8_lossy(&binding);
        assert!(!logged.contains(channel));
        assert!(logged.contains("channel_id="));
    }

    #[tokio::test]
    async fn overload_logs_are_safe_and_structured() {
        let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let writer = SharedWriter(buf.clone());
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = set_default(subscriber);

        let channel = "route-token-super-sensitive-aaaaaaaaaaaaaaaaaaaa";
        let (base, handle) = spawn_server(Limits {
            max_body_bytes: 1024 * 1024,
            max_queue_depth: 1,
        })
        .await;
        let client = reqwest::Client::new();

        let first = canonical_push(&client, &base, channel, b"a".to_vec()).await;
        assert_eq!(first.status(), ReqStatus::OK);

        let second = canonical_push(&client, &base, channel, b"b".to_vec()).await;
        assert_eq!(second.status(), ReqStatus::TOO_MANY_REQUESTS);
        let body = second.text().await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body, "ERR_OVERLOADED");

        handle.abort();

        let binding = buf.lock().unwrap_or_else(|e| panic!("{e}"));
        let logged = String::from_utf8_lossy(&binding);
        assert!(logged.contains("event=overloaded"));
        assert!(logged.contains("queue_depth="));
        assert!(logged.contains("max="));
        assert!(!logged.contains(channel));
        let mut run = 0usize;
        let mut has_long_hex = false;
        for ch in logged.chars() {
            if (ch.is_ascii_hexdigit() && ch.is_ascii_lowercase()) || ch.is_ascii_digit() {
                run += 1;
                if run >= 32 {
                    has_long_hex = true;
                    break;
                }
            } else {
                run = 0;
            }
        }
        assert!(!has_long_hex);
    }

    #[tokio::test]
    async fn auth_disabled_allows_push_pull() {
        let (base, handle) = spawn_server_with_token(
            Limits {
                max_body_bytes: 1024 * 1024,
                max_queue_depth: 8,
            },
            None,
        )
        .await;
        let client = reqwest::Client::new();
        let payload = b"auth-disabled".to_vec();

        let push = canonical_push(&client, &base, "open", payload.clone()).await;
        assert_eq!(push.status(), ReqStatus::OK);

        let pull = canonical_pull(&client, &base, "open", 1).await;
        assert_eq!(pull.status(), ReqStatus::OK);
        let body: PullResp = pull.json().await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body.items.len(), 1);
        assert_eq!(body.items[0].data.as_slice(), payload.as_slice());
        handle.abort();
    }

    #[tokio::test]
    async fn auth_enabled_missing_token_401_no_mutation() {
        let (base, handle) = spawn_server_with_token(
            Limits {
                max_body_bytes: 1024 * 1024,
                max_queue_depth: 8,
            },
            Some("topsecret".to_string()),
        )
        .await;
        let client = reqwest::Client::new();

        let push = client
            .post(format!("{}/v1/push", base))
            .header(ROUTE_TOKEN_HEADER, "auth")
            .body(b"x".to_vec())
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(push.status(), ReqStatus::UNAUTHORIZED);
        assert_eq!(
            push.text().await.unwrap_or_else(|e| panic!("{e}")),
            "ERR_UNAUTHORIZED"
        );

        let pull = client
            .get(format!("{}/v1/pull?max=1", base))
            .header(ROUTE_TOKEN_HEADER, "auth")
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pull.status(), ReqStatus::UNAUTHORIZED);
        assert_eq!(
            pull.text().await.unwrap_or_else(|e| panic!("{e}")),
            "ERR_UNAUTHORIZED"
        );

        let pull_ok = client
            .get(format!("{}/v1/pull?max=1", base))
            .header(ROUTE_TOKEN_HEADER, "auth")
            .header("Authorization", "Bearer topsecret")
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pull_ok.status(), ReqStatus::NO_CONTENT);
        handle.abort();
    }

    #[tokio::test]
    async fn auth_enabled_wrong_token_401_no_mutation() {
        let (base, handle) = spawn_server_with_token(
            Limits {
                max_body_bytes: 1024 * 1024,
                max_queue_depth: 8,
            },
            Some("topsecret".to_string()),
        )
        .await;
        let client = reqwest::Client::new();
        let push = client
            .post(format!("{}/v1/push", base))
            .header(ROUTE_TOKEN_HEADER, "auth")
            .header("Authorization", "Bearer wrong")
            .body(b"x".to_vec())
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(push.status(), ReqStatus::UNAUTHORIZED);
        assert_eq!(
            push.text().await.unwrap_or_else(|e| panic!("{e}")),
            "ERR_UNAUTHORIZED"
        );

        let pull_ok = client
            .get(format!("{}/v1/pull?max=1", base))
            .header(ROUTE_TOKEN_HEADER, "auth")
            .header("Authorization", "Bearer topsecret")
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pull_ok.status(), ReqStatus::NO_CONTENT);
        handle.abort();
    }

    #[tokio::test]
    async fn auth_enabled_correct_token_allows_roundtrip() {
        let (base, handle) = spawn_server_with_token(
            Limits {
                max_body_bytes: 1024 * 1024,
                max_queue_depth: 8,
            },
            Some("topsecret".to_string()),
        )
        .await;
        let client = reqwest::Client::new();
        let payload = b"auth-enabled".to_vec();

        let push = client
            .post(format!("{}/v1/push", base))
            .header(ROUTE_TOKEN_HEADER, "authok")
            .header("Authorization", "Bearer topsecret")
            .body(payload.clone())
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(push.status(), ReqStatus::OK);

        let pull = client
            .get(format!("{}/v1/pull?max=1", base))
            .header(ROUTE_TOKEN_HEADER, "authok")
            .header("Authorization", "Bearer topsecret")
            .send()
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(pull.status(), ReqStatus::OK);
        let body: PullResp = pull.json().await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body.items.len(), 1);
        assert_eq!(body.items[0].data.as_slice(), payload.as_slice());
        handle.abort();
    }
}
