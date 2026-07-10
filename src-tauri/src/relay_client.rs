use crate::{lan_server, AppSettings};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    thread,
    time::Duration,
};
use tungstenite::{
    client::IntoClientRequest, error::Error as WebSocketError, http::header::AUTHORIZATION, Message,
};

const RELAY_POLL_INTERVAL: Duration = Duration::from_secs(2);
const RELAY_RECONNECT_INTERVAL: Duration = Duration::from_secs(5);

static RUNTIME_STATE: OnceLock<Mutex<RelayRuntimeState>> = OnceLock::new();

#[derive(Clone, Default)]
pub struct RelaySnapshot {
    pub connected: bool,
    pub message: String,
}

#[derive(Default)]
struct RelayRuntimeState {
    started: bool,
    connected: bool,
    message: String,
}

#[derive(Serialize)]
struct DesktopResponse {
    id: String,
    #[serde(rename = "type")]
    message_type: &'static str,
    status: u16,
    headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Deserialize)]
struct DesktopRequest {
    id: String,
    #[serde(rename = "type")]
    message_type: String,
    method: String,
    path: String,
    #[serde(default)]
    headers: HashMap<String, String>,
}

#[derive(Serialize)]
struct PairingStartRequest<'a> {
    #[serde(rename = "deviceId")]
    device_id: &'a str,
    #[serde(rename = "deviceName")]
    device_name: &'a str,
    #[serde(rename = "deviceSecret")]
    device_secret: &'a str,
    #[serde(rename = "pairingCode")]
    pairing_code: &'a str,
}

#[derive(Deserialize)]
struct PairingStartResponse {
    #[serde(rename = "remoteUrl")]
    remote_url: Option<String>,
}

pub fn start(app: tauri::AppHandle) {
    let state = runtime_state();
    let mut guard = state.lock().expect("relay runtime state lock poisoned");
    if guard.started {
        return;
    }
    guard.started = true;
    drop(guard);

    thread::spawn(move || loop {
        let settings = match crate::load_app_settings_internal(app.clone()) {
            Ok(settings) => settings,
            Err(error) => {
                update_state(false, format!("Could not load relay settings: {error}"));
                thread::sleep(RELAY_POLL_INTERVAL);
                continue;
            }
        };

        if !settings.enable_hosted_relay {
            update_state(false, "ArchiveKong Remote Access is disabled.".to_string());
            thread::sleep(RELAY_POLL_INTERVAL);
            continue;
        }

        if settings.relay_device_id.is_empty() || settings.relay_device_secret.is_empty() {
            update_state(
                false,
                "Create a pairing before this desktop can connect to ArchiveKong cloud."
                    .to_string(),
            );
            thread::sleep(RELAY_POLL_INTERVAL);
            continue;
        }

        let connection_message = connect_loop(&app, &settings);
        update_state(false, connection_message);
        thread::sleep(RELAY_RECONNECT_INTERVAL);
    });
}

pub fn current_status() -> RelaySnapshot {
    let guard = runtime_state()
        .lock()
        .expect("relay runtime state lock poisoned");
    RelaySnapshot {
        connected: guard.connected,
        message: guard.message.clone(),
    }
}

pub fn register_pairing(settings: &AppSettings) -> Result<Option<String>, String> {
    let relay_url = settings.relay_url.trim_end_matches('/');
    let endpoint = format!("{relay_url}/v1/devices/pair/start");
    let response = ureq::post(&endpoint)
        .set("Content-Type", "application/json")
        .send_json(serde_json::json!(PairingStartRequest {
            device_id: &settings.relay_device_id,
            device_name: &settings.relay_device_name,
            device_secret: &settings.relay_device_secret,
            pairing_code: &settings.relay_pairing_code,
        }))
        .map_err(|error| format!("Could not register pairing with relay: {error}"))?;

    let parsed: PairingStartResponse = response
        .into_json()
        .map_err(|error| format!("Could not read relay pairing response: {error}"))?;
    Ok(parsed.remote_url)
}

fn connect_loop(app: &tauri::AppHandle, settings: &AppSettings) -> String {
    update_state(
        false,
        format!("Connecting to {}", settings.relay_url.trim_end_matches('/')),
    );

    let websocket_url = websocket_url(settings);
    let mut request = match websocket_url.clone().into_client_request() {
        Ok(request) => request,
        Err(error) => {
            return format!("Could not create relay WebSocket request: {error}");
        }
    };

    let auth_value = match format!("Bearer {}", settings.relay_device_secret).parse() {
        Ok(value) => value,
        Err(error) => return format!("Could not build relay authorization header: {error}"),
    };
    request.headers_mut().insert(AUTHORIZATION, auth_value);

    let (mut socket, _) = match tungstenite::connect(request) {
        Ok(result) => result,
        Err(WebSocketError::Http(response)) => {
            let status = response.status();
            if status.as_u16() == 401 {
                return "Relay rejected the desktop connection. Pairing may still be pending."
                    .to_string();
            }
            return format!("Relay rejected the desktop connection with HTTP {status}.");
        }
        Err(error) => return format!("Could not connect to relay: {error}"),
    };

    update_state(true, "Connected to ArchiveKong relay.".to_string());

    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                let response = handle_text_request(app, &text);
                let json = match serde_json::to_string(&response) {
                    Ok(json) => json,
                    Err(error) => {
                        update_state(
                            false,
                            format!("Could not serialize relay response: {error}"),
                        );
                        return "Relay session ended after a serialization failure.".to_string();
                    }
                };
                if let Err(error) = socket.send(Message::Text(json)) {
                    return format!("Relay connection closed while sending a response: {error}");
                }
            }
            Ok(Message::Ping(payload)) => {
                if let Err(error) = socket.send(Message::Pong(payload)) {
                    return format!("Relay ping/pong failed: {error}");
                }
            }
            Ok(Message::Close(_)) => return "Relay connection closed.".to_string(),
            Ok(_) => {}
            Err(error) => return format!("Relay connection error: {error}"),
        }
    }
}

fn handle_text_request(app: &tauri::AppHandle, text: &str) -> DesktopResponse {
    let request = match serde_json::from_str::<DesktopRequest>(text) {
        Ok(request) => request,
        Err(error) => {
            return DesktopResponse {
                id: String::new(),
                message_type: "response",
                status: 400,
                headers: text_headers(),
                body_base64: None,
                error: Some(format!("Invalid relay request JSON: {error}")),
            };
        }
    };

    if request.message_type != "request" {
        return DesktopResponse {
            id: request.id,
            message_type: "response",
            status: 400,
            headers: text_headers(),
            body_base64: None,
            error: Some("Unsupported relay message type.".to_string()),
        };
    }
    if request.method != "GET" {
        return DesktopResponse {
            id: request.id,
            message_type: "response",
            status: 405,
            headers: text_headers(),
            body_base64: None,
            error: Some("Read-only relay only supports GET.".to_string()),
        };
    }

    let relay_headers = lan_server::RelayRequestHeaders {
        range: request.headers.get("range").cloned(),
    };
    match lan_server::handle_relay_get(app, &request.path, &relay_headers) {
        Ok(response) => DesktopResponse {
            id: request.id,
            message_type: "response",
            status: response.status,
            headers: response.headers.into_iter().collect(),
            body_base64: Some(general_purpose::STANDARD.encode(response.body)),
            error: None,
        },
        Err(error) => DesktopResponse {
            id: request.id,
            message_type: "response",
            status: 500,
            headers: text_headers(),
            body_base64: None,
            error: Some(error),
        },
    }
}

fn websocket_url(settings: &AppSettings) -> String {
    let base = settings.relay_url.trim_end_matches('/');
    let scheme = if base.starts_with("https://") {
        "wss://"
    } else if base.starts_with("http://") {
        "ws://"
    } else {
        "wss://"
    };
    let host = base
        .strip_prefix("https://")
        .or_else(|| base.strip_prefix("http://"))
        .unwrap_or(base);
    format!(
        "{scheme}{host}/v1/devices/{}/connect",
        settings.relay_device_id
    )
}

fn text_headers() -> HashMap<String, String> {
    HashMap::from([(
        "content-type".to_string(),
        "text/plain; charset=utf-8".to_string(),
    )])
}

fn update_state(connected: bool, message: String) {
    let mut guard = runtime_state()
        .lock()
        .expect("relay runtime state lock poisoned");
    guard.connected = connected;
    guard.message = message;
}

fn runtime_state() -> &'static Mutex<RelayRuntimeState> {
    RUNTIME_STATE.get_or_init(|| Mutex::new(RelayRuntimeState::default()))
}
