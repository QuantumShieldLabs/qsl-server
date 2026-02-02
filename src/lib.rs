use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
use tracing::info;
use uuid::Uuid;

// Named aliases to keep queue types readable (also satisfies clippy::type_complexity).
type QueueMsg = (String, Vec<u8>);
type Queue = VecDeque<QueueMsg>;
type Queues = HashMap<String, Queue>;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_body_bytes: usize,
    pub max_queue_depth: usize,
}

pub const MAX_BODY_BYTES_CEILING: usize = 1024 * 1024;
pub const MAX_QUEUE_DEPTH_CEILING: usize = 256;

impl Limits {
    pub fn from_env() -> Self {
        let max_body_bytes = std::env::var("MAX_BODY_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(MAX_BODY_BYTES_CEILING)
            .min(MAX_BODY_BYTES_CEILING);
        let max_queue_depth = std::env::var("MAX_QUEUE_DEPTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(MAX_QUEUE_DEPTH_CEILING)
            .min(MAX_QUEUE_DEPTH_CEILING);
        Self {
            max_body_bytes,
            max_queue_depth,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    // channel -> queue of (msg_id, bytes)
    queues: Arc<Mutex<Queues>>,

    limits: Limits,
}

impl AppState {
    pub fn new(limits: Limits) -> Self {
        Self {
            queues: Arc::new(Mutex::new(HashMap::new())),
            limits,
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
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/v1/push/:channel", post(push_message))
        .route("/v1/pull/:channel", get(pull_message))
        .with_state(state)
}

async fn push_message(
    State(st): State<AppState>,
    Path(channel): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
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
    let mut g = st.queues.lock().unwrap();
    let q = g.entry(channel.clone()).or_default();
    if q.len() >= st.limits.max_queue_depth {
        return (StatusCode::TOO_MANY_REQUESTS, "ERR_QUEUE_FULL").into_response();
    }
    q.push_back((msg_id.clone(), body.to_vec()));

    // Never log payload; metadata only.
    info!(
        "push channel={} id={} bytes={}",
        channel,
        msg_id,
        body.len()
    );

    (StatusCode::OK, Json(PostResp { id: msg_id })).into_response()
}

async fn pull_message(
    State(st): State<AppState>,
    Path(channel): Path<String>,
    Query(query): Query<PullQuery>,
) -> impl IntoResponse {
    let max = query.max.unwrap_or(1);
    if max == 0 {
        return (StatusCode::BAD_REQUEST, "ERR_BAD_MAX").into_response();
    }
    let max = max.min(st.limits.max_queue_depth);
    let mut g = st.queues.lock().unwrap();
    let q = g.entry(channel.clone()).or_default();
    if q.is_empty() {
        return StatusCode::NO_CONTENT.into_response();
    }
    let mut items = Vec::with_capacity(max);
    for _ in 0..max {
        if let Some((msg_id, data)) = q.pop_front() {
            info!(
                "pull channel={} id={} bytes={}",
                channel,
                msg_id,
                data.len()
            );
            items.push(PullItem { id: msg_id, data });
        } else {
            break;
        }
    }
    (StatusCode::OK, Json(PullResp { items })).into_response()
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
            let mut g = self.0.lock().unwrap();
            g.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    async fn spawn_server(limits: Limits) -> (String, tokio::task::JoinHandle<()>) {
        let state = AppState::new(limits);
        let app = app(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{}", addr), handle)
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

    #[tokio::test]
    async fn push_then_pull_roundtrip() {
        let (base, handle) = spawn_server(Limits {
            max_body_bytes: 1024 * 1024,
            max_queue_depth: 8,
        })
        .await;

        let client = reqwest::Client::new();
        let payload = b"opaque-bytes".to_vec();
        let push = client
            .post(format!("{}/v1/push/test", base))
            .body(payload.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(push.status(), ReqStatus::OK);

        let pull = client
            .get(format!("{}/v1/pull/test?max=1", base))
            .send()
            .await
            .unwrap();
        assert_eq!(pull.status(), ReqStatus::OK);
        let body: PullResp = pull.json().await.unwrap();
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
        let pull = client
            .get(format!("{}/v1/pull/empty?max=1", base))
            .send()
            .await
            .unwrap();
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
        let push = client
            .post(format!("{}/v1/push/oversize", base))
            .body(vec![0u8; 5])
            .send()
            .await
            .unwrap();
        assert_eq!(push.status(), ReqStatus::PAYLOAD_TOO_LARGE);
        let body = push.text().await.unwrap();
        assert_eq!(body, "ERR_TOO_LARGE");

        let pull = client
            .get(format!("{}/v1/pull/oversize?max=1", base))
            .send()
            .await
            .unwrap();
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
        let r1 = client
            .post(format!("{}/v1/push/qfull", base))
            .body(b"a".to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(r1.status(), ReqStatus::OK);

        let r2 = client
            .post(format!("{}/v1/push/qfull", base))
            .body(b"b".to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(r2.status(), ReqStatus::TOO_MANY_REQUESTS);
        let body = r2.text().await.unwrap();
        assert_eq!(body, "ERR_QUEUE_FULL");

        let pull1 = client
            .get(format!("{}/v1/pull/qfull?max=1", base))
            .send()
            .await
            .unwrap();
        assert_eq!(pull1.status(), ReqStatus::OK);
        let body1: PullResp = pull1.json().await.unwrap();
        assert_eq!(body1.items.len(), 1);
        assert!(!body1.items[0].id.is_empty());

        let pull2 = client
            .get(format!("{}/v1/pull/qfull?max=1", base))
            .send()
            .await
            .unwrap();
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

        let _ = client
            .post(format!("{}/v1/push/two", base))
            .body(b"a".to_vec())
            .send()
            .await
            .unwrap();
        let _ = client
            .post(format!("{}/v1/push/two", base))
            .body(b"b".to_vec())
            .send()
            .await
            .unwrap();

        let pull1 = client
            .get(format!("{}/v1/pull/two?max=1", base))
            .send()
            .await
            .unwrap();
        assert_eq!(pull1.status(), ReqStatus::OK);
        let body1: PullResp = pull1.json().await.unwrap();
        assert_eq!(body1.items.len(), 1);

        let pull2 = client
            .get(format!("{}/v1/pull/two?max=2", base))
            .send()
            .await
            .unwrap();
        assert_eq!(pull2.status(), ReqStatus::OK);
        let body2: PullResp = pull2.json().await.unwrap();
        assert_eq!(body2.items.len(), 1);
        assert!(!body2.items[0].id.is_empty());

        let pull3 = client
            .get(format!("{}/v1/pull/two?max=1", base))
            .send()
            .await
            .unwrap();
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
        let _ = client
            .post(format!("{}/v1/push/nolog", base))
            .body(payload)
            .send()
            .await
            .unwrap();

        handle.abort();

        let binding = buf.lock().unwrap();
        let logged = String::from_utf8_lossy(&binding);
        assert!(!logged.contains("SECRET_PAYLOAD_ABC"));
    }
}
