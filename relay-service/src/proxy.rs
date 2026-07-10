use crate::{protocol, AppState};
use axum::{
    body::Body,
    extract::ws::{Message, WebSocket},
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, Response, StatusCode},
    response::IntoResponse,
};
use base64::{engine::general_purpose, Engine as _};
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::{
    sync::{mpsc, oneshot, Mutex, RwLock},
    time,
};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct Sessions {
    inner: Arc<RwLock<HashMap<String, DeviceSession>>>,
}

impl Sessions {
    pub async fn insert(&self, device_id: String, session: DeviceSession) {
        self.inner.write().await.insert(device_id, session);
    }

    pub async fn remove(&self, device_id: &str) {
        self.inner.write().await.remove(device_id);
    }

    pub async fn get(&self, device_id: &str) -> Option<DeviceSession> {
        self.inner.read().await.get(device_id).cloned()
    }

    pub async fn is_connected(&self, device_id: &str) -> bool {
        self.inner.read().await.contains_key(device_id)
    }
}

#[derive(Clone)]
pub struct DeviceSession {
    tx: mpsc::Sender<protocol::DesktopRequest>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<protocol::DesktopResponse>>>>,
    active_requests: Arc<AtomicUsize>,
}

pub async fn connect_desktop(device_id: String, socket: WebSocket, sessions: Sessions) {
    let (mut socket_tx, mut socket_rx) = socket.split();
    let (tx, mut rx) = mpsc::channel::<protocol::DesktopRequest>(64);
    let pending = Arc::new(Mutex::new(HashMap::<
        String,
        oneshot::Sender<protocol::DesktopResponse>,
    >::new()));
    let session = DeviceSession {
        tx,
        pending: Arc::clone(&pending),
        active_requests: Arc::new(AtomicUsize::new(0)),
    };

    sessions.insert(device_id.clone(), session).await;
    tracing::info!(device_id, "desktop connected");

    let writer_device_id = device_id.clone();
    let writer = tokio::spawn(async move {
        while let Some(request) = rx.recv().await {
            let Ok(json) = serde_json::to_string(&request) else {
                continue;
            };
            if socket_tx.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
        tracing::info!(device_id = writer_device_id, "desktop writer stopped");
    });

    while let Some(message) = socket_rx.next().await {
        match message {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<protocol::DesktopResponse>(&text) {
                    Ok(response) => {
                        if response.message_type != "response" {
                            tracing::warn!(device_id, "ignored unexpected desktop message");
                            continue;
                        }
                        let sender = pending.lock().await.remove(&response.id);
                        if let Some(sender) = sender {
                            let _ = sender.send(response);
                        }
                    }
                    Err(error) => tracing::warn!(device_id, %error, "invalid desktop response"),
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(device_id, %error, "desktop websocket error");
                break;
            }
        }
    }

    writer.abort();
    sessions.remove(&device_id).await;
    tracing::info!(device_id, "desktop disconnected");
}

pub async fn proxy_request(
    state: AppState,
    device_id: String,
    path: String,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ProxyError> {
    let session = state
        .sessions
        .get(&device_id)
        .await
        .ok_or(ProxyError::Disconnected)?;

    let current_requests = session.active_requests.fetch_add(1, Ordering::SeqCst);
    if current_requests >= state.limits.max_concurrent_requests {
        session.active_requests.fetch_sub(1, Ordering::SeqCst);
        return Err(ProxyError::TooManyRequests);
    }

    let result = send_desktop_request(session.clone(), path, headers, state).await;
    session.active_requests.fetch_sub(1, Ordering::SeqCst);
    result
}

async fn send_desktop_request(
    session: DeviceSession,
    path: String,
    headers: HeaderMap,
    state: AppState,
) -> Result<Response<Body>, ProxyError> {
    let request_id = Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    session.pending.lock().await.insert(request_id.clone(), tx);

    let request = protocol::DesktopRequest {
        id: request_id.clone(),
        message_type: "request",
        method: Method::GET.to_string(),
        path,
        headers: relay_headers(&headers),
    };

    if session.tx.send(request).await.is_err() {
        session.pending.lock().await.remove(&request_id);
        return Err(ProxyError::Disconnected);
    }

    let response = match time::timeout(state.limits.request_timeout, rx).await {
        Ok(Ok(response)) => response,
        Ok(Err(_)) => return Err(ProxyError::Disconnected),
        Err(_) => {
            session.pending.lock().await.remove(&request_id);
            return Err(ProxyError::TimedOut);
        }
    };

    if let Some(error) = response.error {
        return Err(ProxyError::BadResponse(error));
    }

    let body = match response.body_base64 {
        Some(encoded) => general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| ProxyError::BadResponse(format!("Invalid base64 body: {error}")))?,
        None => Vec::new(),
    };
    if body.len() > state.limits.max_body_bytes {
        return Err(ProxyError::ResponseTooLarge);
    }

    let status = StatusCode::from_u16(response.status)
        .map_err(|error| ProxyError::BadResponse(format!("Invalid response status: {error}")))?;
    let mut builder = Response::builder().status(status);
    for (name, value) in filtered_response_headers(response.headers) {
        builder = builder.header(name, value);
    }

    builder
        .body(Body::from(body))
        .map_err(|error| ProxyError::BadResponse(format!("Could not build response: {error}")))
}

fn relay_headers(headers: &HeaderMap) -> HashMap<String, String> {
    let mut relayed = HashMap::new();
    for name in [
        header::RANGE,
        header::ACCEPT,
        header::ACCEPT_LANGUAGE,
        header::USER_AGENT,
    ] {
        if let Some(value) = headers.get(&name).and_then(|value| value.to_str().ok()) {
            relayed.insert(name.as_str().to_string(), value.to_string());
        }
    }
    relayed
}

fn filtered_response_headers(headers: HashMap<String, String>) -> Vec<(HeaderName, HeaderValue)> {
    let allowed = [
        "accept-ranges",
        "cache-control",
        "content-disposition",
        "content-length",
        "content-range",
        "content-type",
    ];
    headers
        .into_iter()
        .filter_map(|(name, value)| {
            let lower_name = name.to_ascii_lowercase();
            if !allowed.contains(&lower_name.as_str()) {
                return None;
            }
            let name = HeaderName::from_bytes(lower_name.as_bytes()).ok()?;
            let value = HeaderValue::from_str(&value).ok()?;
            Some((name, value))
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("desktop disconnected")]
    Disconnected,
    #[error("desktop response timed out")]
    TimedOut,
    #[error("too many active relay requests")]
    TooManyRequests,
    #[error("response too large")]
    ResponseTooLarge,
    #[error("{0}")]
    BadResponse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relays_range_header_only_from_request_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, HeaderValue::from_static("bytes=0-99"));
        headers.insert(header::HOST, HeaderValue::from_static("example.com"));

        let relayed = relay_headers(&headers);

        assert_eq!(relayed.get("range").map(String::as_str), Some("bytes=0-99"));
        assert!(!relayed.contains_key("host"));
    }

    #[test]
    fn filters_response_headers_to_streaming_safe_subset() {
        let headers = HashMap::from([
            ("Content-Type".to_string(), "video/mp4".to_string()),
            ("Content-Range".to_string(), "bytes 0-99/100".to_string()),
            ("Set-Cookie".to_string(), "bad=true".to_string()),
        ]);

        let filtered = filtered_response_headers(headers);
        let names = filtered
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();

        assert!(names.contains(&"content-type"));
        assert!(names.contains(&"content-range"));
        assert!(!names.contains(&"set-cookie"));
    }
}
