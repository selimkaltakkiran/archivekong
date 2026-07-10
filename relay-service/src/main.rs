mod devices;
mod limits;
mod protocol;
mod proxy;

use axum::{
    extract::{Form, Path, Query, State, WebSocketUpgrade},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use devices::{AuthRequest, ConfirmPairingRequest, DeviceStore, PairingStartRequest, User};
use limits::RelayLimits;
use proxy::{connect_desktop, proxy_request, Sessions};
use serde::Deserialize;
use std::{env, net::SocketAddr, sync::Arc, time::Duration};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const SESSION_COOKIE_NAME: &str = "archivekong_session";

#[derive(Clone)]
struct AppState {
    device_store: Arc<DeviceStore>,
    sessions: Sessions,
    public_url: String,
    limits: RelayLimits,
    secure_cookie: bool,
}

#[derive(Deserialize)]
struct FlashQuery {
    message: Option<String>,
}

#[derive(Deserialize)]
struct AuthForm {
    email: String,
    password: String,
}

#[derive(Deserialize)]
struct PairForm {
    pairing_code: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "archivekong_relay_service=info,tower_http=info,axum=info".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let bind = env::var("ARCHIVEKONG_RELAY_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let database_path =
        env::var("ARCHIVEKONG_RELAY_DATABASE").unwrap_or_else(|_| "relay.sqlite".into());
    let public_url =
        env::var("ARCHIVEKONG_RELAY_PUBLIC_URL").unwrap_or_else(|_| format!("http://{bind}"));
    let max_concurrent_requests = env::var("ARCHIVEKONG_RELAY_MAX_CONCURRENT_REQUESTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(8);
    let request_timeout_seconds = env::var("ARCHIVEKONG_RELAY_REQUEST_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30);
    let max_body_bytes = env::var("ARCHIVEKONG_RELAY_MAX_BODY_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(64 * 1024 * 1024);

    let state = AppState {
        device_store: Arc::new(DeviceStore::open(database_path)?),
        sessions: Sessions::default(),
        secure_cookie: public_url.starts_with("https://"),
        public_url,
        limits: RelayLimits {
            max_concurrent_requests,
            request_timeout: Duration::from_secs(request_timeout_seconds),
            max_body_bytes,
        },
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/login", get(login_page).post(login_submit))
        .route("/register", get(register_page).post(register_submit))
        .route("/logout", post(logout_submit))
        .route("/devices", get(devices_page))
        .route("/pair", get(pair_page).post(pair_submit))
        .route("/v1/auth/register", post(register_api))
        .route("/v1/auth/login", post(login_api))
        .route("/v1/auth/logout", post(logout_api))
        .route("/v1/auth/session", get(session_api))
        .route("/v1/devices/pair/start", post(start_pairing))
        .route("/v1/devices/pair/confirm", post(confirm_pairing_api))
        .route("/v1/devices/:device_id/status", get(device_status))
        .route("/v1/devices/:device_id/connect", get(connect))
        .route("/remote/:device_id", get(remote_shell))
        .route("/remote/:device_id/api/library", get(remote_library))
        .route("/remote/:device_id/api/image", get(remote_image))
        .route("/remote/:device_id/api/media", get(remote_media))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let address: SocketAddr = bind.parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!("ArchiveKong relay listening on http://{address}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn root(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, RelayHttpError> {
    if current_user(&state, &headers).is_ok() {
        Ok(Redirect::to("/devices").into_response())
    } else {
        Ok(Redirect::to("/login").into_response())
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "archivekong-relay-service"
    }))
}

async fn login_page(Query(query): Query<FlashQuery>) -> Html<String> {
    auth_page("Sign in", "/login", "Sign in", query.message.as_deref())
}

async fn register_page(Query(query): Query<FlashQuery>) -> Html<String> {
    auth_page(
        "Create account",
        "/register",
        "Create account",
        query.message.as_deref(),
    )
}

async fn devices_page(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Html<String>, RelayHttpError> {
    let user = current_user(&state, &headers)?;
    let mut devices = state.device_store.list_devices_for_user(user.id)?;
    for device in &mut devices {
        device.connected = state.sessions.is_connected(&device.device_id).await;
    }

    let items = if devices.is_empty() {
        "<p>No devices yet. Pair one from the ArchiveKong desktop app.</p>".to_string()
    } else {
        let mut html = String::from("<ul>");
        for device in devices {
            html.push_str(&format!(
                r#"<li><strong>{}</strong> - {} - <a href="/remote/{}">Open</a></li>"#,
                html_escape(&device.device_name),
                if device.connected {
                    "online"
                } else {
                    "offline"
                },
                device.device_id
            ));
        }
        html.push_str("</ul>");
        html
    };

    Ok(Html(page_shell(
        "My devices",
        &format!(
            r#"<header><h1>My devices</h1><p>{}</p></header>
<nav><a href="/pair">Pair a device</a></nav>
{}
<form method="post" action="/logout"><button type="submit">Sign out</button></form>"#,
            html_escape(&user.email),
            items
        ),
    )))
}

async fn pair_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<FlashQuery>,
) -> Result<Html<String>, RelayHttpError> {
    let user = current_user(&state, &headers)?;
    let body = format!(
        r#"<header><h1>Pair device</h1><p>Signed in as {}</p></header>
<form method="post" action="/pair">
  <label>Pairing code<input type="text" name="pairing_code" placeholder="ABCDE-12345" /></label>
  <button type="submit">Confirm pairing</button>
</form>
{}
<p><a href="/devices">Back to devices</a></p>"#,
        html_escape(&user.email),
        flash_markup(query.message.as_deref())
    );
    Ok(Html(page_shell("Pair device", &body)))
}

async fn login_submit(
    State(state): State<AppState>,
    Form(form): Form<AuthForm>,
) -> Result<Response, RelayHttpError> {
    let user = state.device_store.authenticate_user(&AuthRequest {
        email: form.email,
        password: form.password,
    })?;
    let session = state.device_store.create_session(user.id)?;
    Ok(with_session_cookie(
        Redirect::to("/devices").into_response(),
        &session,
        state.secure_cookie,
    ))
}

async fn register_submit(
    State(state): State<AppState>,
    Form(form): Form<AuthForm>,
) -> Result<Response, RelayHttpError> {
    let user = state.device_store.create_user(&AuthRequest {
        email: form.email,
        password: form.password,
    })?;
    let session = state.device_store.create_session(user.id)?;
    Ok(with_session_cookie(
        Redirect::to("/devices").into_response(),
        &session,
        state.secure_cookie,
    ))
}

async fn pair_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<PairForm>,
) -> Result<Response, RelayHttpError> {
    let user = current_user(&state, &headers)?;
    let device = state
        .device_store
        .confirm_pairing(&form.pairing_code, user.id)?;
    Ok(Redirect::to(&format!("/remote/{}", device.device_id)).into_response())
}

async fn logout_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, RelayHttpError> {
    if let Some(session_id) = session_cookie(&headers) {
        state.device_store.delete_session(&session_id)?;
    }
    Ok(clear_session_cookie(
        Redirect::to("/login").into_response(),
        state.secure_cookie,
    ))
}

async fn register_api(
    State(state): State<AppState>,
    Json(request): Json<AuthRequest>,
) -> Result<Response, RelayHttpError> {
    let user = state.device_store.create_user(&request)?;
    let session = state.device_store.create_session(user.id)?;
    let response = Json(serde_json::json!({
        "user": user
    }))
    .into_response();
    Ok(with_session_cookie(response, &session, state.secure_cookie))
}

async fn login_api(
    State(state): State<AppState>,
    Json(request): Json<AuthRequest>,
) -> Result<Response, RelayHttpError> {
    let user = state.device_store.authenticate_user(&request)?;
    let session = state.device_store.create_session(user.id)?;
    let response = Json(serde_json::json!({
        "user": user
    }))
    .into_response();
    Ok(with_session_cookie(response, &session, state.secure_cookie))
}

async fn logout_api(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, RelayHttpError> {
    if let Some(session_id) = session_cookie(&headers) {
        state.device_store.delete_session(&session_id)?;
    }
    let response = Json(serde_json::json!({
        "status": "ok"
    }))
    .into_response();
    Ok(clear_session_cookie(response, state.secure_cookie))
}

async fn session_api(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, RelayHttpError> {
    let user = current_user(&state, &headers)?;
    Ok(Json(serde_json::json!({
        "user": user
    })))
}

async fn start_pairing(
    State(state): State<AppState>,
    Json(request): Json<PairingStartRequest>,
) -> Result<Json<serde_json::Value>, RelayHttpError> {
    state.device_store.start_pairing(&request)?;
    Ok(Json(serde_json::json!({
        "status": "pending",
        "deviceId": request.device_id,
        "remoteUrl": format!("{}/remote/{}", state.public_url.trim_end_matches('/'), request.device_id)
    })))
}

async fn confirm_pairing_api(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ConfirmPairingRequest>,
) -> Result<Json<serde_json::Value>, RelayHttpError> {
    let user = current_user(&state, &headers)?;
    let device = state
        .device_store
        .confirm_pairing(&request.pairing_code, user.id)?;
    Ok(Json(serde_json::json!({
        "status": "paired",
        "deviceId": device.device_id,
        "deviceName": device.device_name,
        "remoteUrl": format!("{}/remote/{}", state.public_url.trim_end_matches('/'), device.device_id)
    })))
}

async fn device_status(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, RelayHttpError> {
    let user = current_user(&state, &headers)?;
    let device = state.device_store.owned_device(&device_id, user.id)?;
    Ok(Json(serde_json::json!({
        "deviceId": device.device_id,
        "deviceName": device.device_name,
        "paired": device.paired,
        "revoked": device.revoked,
        "connected": state.sessions.is_connected(&device_id).await
    })))
}

async fn connect(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    websocket: WebSocketUpgrade,
    headers: HeaderMap,
) -> Result<impl IntoResponse, RelayHttpError> {
    let secret = bearer_token(&headers).ok_or(RelayHttpError::unauthorized(
        "Missing desktop bearer token.",
    ))?;
    state.device_store.authenticate(&device_id, &secret)?;

    Ok(websocket
        .on_upgrade(move |socket| connect_desktop(device_id, socket, state.sessions.clone())))
}

async fn remote_shell(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Result<Html<String>, RelayHttpError> {
    let user = current_user(&state, &headers)?;
    let device = state.device_store.owned_device(&device_id, user.id)?;
    Ok(Html(page_shell(
        "ArchiveKong Remote",
        &format!(
            r#"<header><h1>{}</h1><p>Signed in as {}</p></header>
<p>Status: {}</p>
<ul>
  <li><a href="/remote/{}/api/library">Library JSON</a></li>
  <li>Images and media stream through the same remote device path.</li>
</ul>
<p><a href="/devices">Back to devices</a></p>"#,
            html_escape(&device.device_name),
            html_escape(&user.email),
            if state.sessions.is_connected(&device_id).await {
                "desktop online"
            } else {
                "desktop offline"
            },
            device_id
        ),
    )))
}

async fn remote_library(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, RelayHttpError> {
    let user = current_user(&state, &headers)?;
    state.device_store.owned_device(&device_id, user.id)?;
    proxy_request(state, device_id, "/api/library".to_string(), headers)
        .await
        .map(IntoResponse::into_response)
        .map_err(Into::into)
}

async fn remote_image(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    query: Query<HashQuery>,
    headers: HeaderMap,
) -> Result<Response, RelayHttpError> {
    let user = current_user(&state, &headers)?;
    state.device_store.owned_device(&device_id, user.id)?;
    let query = query_to_string(query.0.path.as_deref());
    proxy_request(state, device_id, format!("/api/image{query}"), headers)
        .await
        .map(IntoResponse::into_response)
        .map_err(Into::into)
}

async fn remote_media(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    query: Query<HashQuery>,
    headers: HeaderMap,
) -> Result<Response, RelayHttpError> {
    let user = current_user(&state, &headers)?;
    state.device_store.owned_device(&device_id, user.id)?;
    let query = query_to_string(query.0.path.as_deref());
    proxy_request(state, device_id, format!("/api/media{query}"), headers)
        .await
        .map(IntoResponse::into_response)
        .map_err(Into::into)
}

#[derive(Deserialize)]
struct HashQuery {
    path: Option<String>,
}

fn query_to_string(path: Option<&str>) -> String {
    match path {
        Some(path) => format!("?path={}", url_encode(path)),
        None => String::new(),
    }
}

fn current_user(state: &AppState, headers: &HeaderMap) -> Result<User, RelayHttpError> {
    let session = session_cookie(headers)
        .ok_or_else(|| RelayHttpError::unauthorized("You must sign in first."))?;
    state
        .device_store
        .user_for_session(&session)
        .map_err(Into::into)
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let (name, value) = part.trim().split_once('=')?;
        if name == SESSION_COOKIE_NAME {
            return Some(value.to_string());
        }
    }
    None
}

fn with_session_cookie(mut response: Response, session_id: &str, secure_cookie: bool) -> Response {
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&build_session_cookie(session_id, secure_cookie))
            .expect("valid set-cookie header"),
    );
    response
}

fn clear_session_cookie(mut response: Response, secure_cookie: bool) -> Response {
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}",
            SESSION_COOKIE_NAME,
            if secure_cookie { "; Secure" } else { "" }
        ))
        .expect("valid set-cookie header"),
    );
    response
}

fn build_session_cookie(session_id: &str, secure_cookie: bool) -> String {
    format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        SESSION_COOKIE_NAME,
        session_id,
        60 * 60 * 24 * 14,
        secure = if secure_cookie { "; Secure" } else { "" }
    )
}

fn auth_page(title: &str, action: &str, button: &str, message: Option<&str>) -> Html<String> {
    Html(page_shell(
        title,
        &format!(
            r#"<header><h1>{}</h1></header>
<form method="post" action="{}">
  <label>Email<input type="email" name="email" /></label>
  <label>Password<input type="password" name="password" /></label>
  <button type="submit">{}</button>
</form>
{}
<p><a href="/login">Sign in</a> or <a href="/register">create account</a>.</p>"#,
            title,
            action,
            button,
            flash_markup(message)
        ),
    ))
}

fn flash_markup(message: Option<&str>) -> String {
    match message {
        Some(message) if !message.is_empty() => {
            format!(r#"<p class="flash">{}</p>"#, html_escape(message))
        }
        _ => String::new(),
    }
}

fn page_shell(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{}</title>
  <style>
    :root {{ color-scheme: light; font-family: "Segoe UI", Arial, sans-serif; }}
    body {{ margin: 0; background: #f4efe8; color: #1f1a17; }}
    main {{ max-width: 720px; margin: 48px auto; padding: 24px; background: #fffdf9; border: 1px solid #d9cdbd; border-radius: 8px; }}
    form {{ display: grid; gap: 12px; }}
    label {{ display: grid; gap: 6px; font-size: 0.95rem; }}
    input, button {{ min-height: 38px; font: inherit; }}
    input {{ padding: 8px 10px; border: 1px solid #bfae99; border-radius: 6px; }}
    button {{ padding: 8px 12px; border: 1px solid #7b5c3f; border-radius: 6px; background: #8a6747; color: white; cursor: pointer; }}
    a {{ color: #6a4528; }}
    .flash {{ padding: 10px 12px; background: #f7ebdc; border: 1px solid #d8b991; border-radius: 6px; }}
    ul {{ padding-left: 20px; }}
  </style>
</head>
<body><main>{}</main></body>
</html>"#,
        html_escape(title),
        body
    )
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::to_string)
}

fn url_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[derive(Debug)]
struct RelayHttpError {
    status: StatusCode,
    message: String,
}

impl RelayHttpError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }
}

impl IntoResponse for RelayHttpError {
    fn into_response(self) -> Response {
        if matches!(self.status, StatusCode::UNAUTHORIZED) {
            return Redirect::to(&format!("/login?message={}", url_encode(&self.message)))
                .into_response();
        }
        (
            self.status,
            Json(serde_json::json!({
                "error": self.message
            })),
        )
            .into_response()
    }
}

impl From<devices::DeviceStoreError> for RelayHttpError {
    fn from(error: devices::DeviceStoreError) -> Self {
        match error {
            devices::DeviceStoreError::InvalidInput(message) => Self {
                status: StatusCode::BAD_REQUEST,
                message,
            },
            devices::DeviceStoreError::Unauthorized(message) => Self {
                status: StatusCode::UNAUTHORIZED,
                message,
            },
            devices::DeviceStoreError::NotFound(message) => Self {
                status: StatusCode::NOT_FOUND,
                message,
            },
            devices::DeviceStoreError::PasswordHash(message) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message,
            },
            devices::DeviceStoreError::Database(error) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: format!("Database error: {error}"),
            },
        }
    }
}

impl From<proxy::ProxyError> for RelayHttpError {
    fn from(error: proxy::ProxyError) -> Self {
        match error {
            proxy::ProxyError::Disconnected => Self {
                status: StatusCode::SERVICE_UNAVAILABLE,
                message: "Desktop app is not connected.".to_string(),
            },
            proxy::ProxyError::TimedOut => Self {
                status: StatusCode::GATEWAY_TIMEOUT,
                message: "Desktop app did not respond in time.".to_string(),
            },
            proxy::ProxyError::TooManyRequests => Self {
                status: StatusCode::TOO_MANY_REQUESTS,
                message: "Too many active relay requests for this device.".to_string(),
            },
            proxy::ProxyError::ResponseTooLarge => Self {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                message: "Relayed response exceeded the configured limit.".to_string(),
            },
            proxy::ProxyError::BadResponse(message) => Self {
                status: StatusCode::BAD_GATEWAY,
                message,
            },
        }
    }
}
