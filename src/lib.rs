use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
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

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_body_bytes: usize,
    pub max_queue_depth: usize,
}

impl Limits {
    pub fn from_env() -> Self {
        let max_body_bytes = std::env::var("MAX_BODY_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024 * 1024);
        let max_queue_depth = std::env::var("MAX_QUEUE_DEPTH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(256);
        Self {
            max_body_bytes,
            max_queue_depth,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    // channel -> queue of (msg_id, bytes)
    queues: Arc<Mutex<HashMap<String, VecDeque<(String, Vec<u8>)>>>>,
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

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/v1/push/:channel", post(push_message))
        .route("/v1/pull/:channel", get(pull_message))
        .with_state(state)
}

async fn push_message(
    State(st): State<AppState>,
    Path(channel): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, "ERR_EMPTY_BODY").into_response();
    }
    if body.len() > st.limits.max_body_bytes {
        return (StatusCode::PAYLOAD_TOO_LARGE, "ERR_TOO_LARGE").into_response();
    }

    let msg_id = Uuid::new_v4().to_string();
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
) -> impl IntoResponse {
    let mut g = st.queues.lock().unwrap();
    let q = g.entry(channel.clone()).or_default();
    if let Some((msg_id, data)) = q.pop_front() {
        let mut headers = HeaderMap::new();
        headers.insert("x-msg-id", HeaderValue::from_str(&msg_id).unwrap());
        info!(
            "pull channel={} id={} bytes={}",
            channel,
            msg_id,
            data.len()
        );
        return (StatusCode::OK, headers, data).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode as ReqStatus;
    use tokio::net::TcpListener;

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
            .get(format!("{}/v1/pull/test", base))
            .send()
            .await
            .unwrap();
        assert_eq!(pull.status(), ReqStatus::OK);
        let body = pull.bytes().await.unwrap();
        assert_eq!(body.as_ref(), payload.as_slice());

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
            .get(format!("{}/v1/pull/empty", base))
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
        handle.abort();
    }
}
