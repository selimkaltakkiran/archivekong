use argon2::{
    password_hash::{PasswordHash, PasswordVerifier},
    Argon2,
};
use base64::{engine::general_purpose, Engine as _};
use percent_encoding::percent_decode_str;
use rust_embed::RustEmbed;
use serde_json::Value;
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    net::UdpSocket,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
    thread,
    time::SystemTime,
};
use tauri::Manager;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub const LAN_PORT: u16 = 4545;
static LAN_ACCESS_ENABLED: AtomicBool = AtomicBool::new(true);

#[derive(RustEmbed)]
#[folder = "../dist/"]
struct WebAssets;

pub fn start(app: tauri::AppHandle) {
    thread::spawn(move || {
        let address = format!("0.0.0.0:{LAN_PORT}");
        let server = match Server::http(&address) {
            Ok(server) => server,
            Err(error) => {
                eprintln!("Could not start LAN server on {address}: {error}");
                return;
            }
        };

        println!("ArchiveKong LAN library available at {}", public_url());
        let library_index = Arc::new(Mutex::new(LibraryIndex::default()));
        for request in server.incoming_requests() {
            let app = app.clone();
            let library_index = Arc::clone(&library_index);
            thread::spawn(move || handle_request(request, &app, &library_index));
        }
    });
}

pub fn set_enabled(enabled: bool) {
    LAN_ACCESS_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    LAN_ACCESS_ENABLED.load(Ordering::Relaxed)
}

pub fn public_url() -> String {
    format!(
        "http://{}:{LAN_PORT}",
        local_ip_for("239.255.255.250:1900").unwrap_or_else(|| "127.0.0.1".into())
    )
}

pub fn public_url_for_peer(peer: std::net::SocketAddr) -> String {
    format!(
        "http://{}:{LAN_PORT}",
        local_ip_for(peer).unwrap_or_else(|| "127.0.0.1".into())
    )
}

fn local_ip_for(destination: impl std::net::ToSocketAddrs) -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(destination).ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

fn handle_request(request: Request, app: &tauri::AppHandle, library_index: &Mutex<LibraryIndex>) {
    let url = request.url().to_string();
    let (route, query) = url.split_once('?').unwrap_or((&url, ""));
    let method = request.method().as_str();
    let lan_enabled = is_enabled();
    let dlna_enabled = crate::dlna::is_enabled();

    if method == "POST" && route == "/dlna/control/content-directory" && dlna_enabled {
        crate::dlna::handle_content_directory(request, app);
        return;
    }
    if method == "POST" && route == "/dlna/control/connection-manager" && dlna_enabled {
        crate::dlna::handle_connection_manager(request);
        return;
    }
    if matches!(method, "SUBSCRIBE" | "UNSUBSCRIBE")
        && route.starts_with("/dlna/event/")
        && dlna_enabled
    {
        crate::dlna::handle_subscription(request);
        return;
    }
    if request.method() != &Method::Get && request.method() != &Method::Head {
        respond_text(
            request,
            405,
            "Read-only server: only GET and HEAD are allowed.",
        );
        return;
    }

    if !route.starts_with("/dlna/") && !is_authorized(&request, app) {
        respond_unauthorized(request);
        return;
    }

    match route {
        "/dlna/device.xml" if dlna_enabled => {
            let base_url = request_base_url(&request).unwrap_or_else(public_url);
            respond_xml(request, 200, crate::dlna::device_description(&base_url))
        }
        "/dlna/content-directory.xml" if dlna_enabled => respond_xml(
            request,
            200,
            crate::dlna::content_directory_description().to_string(),
        ),
        "/dlna/connection-manager.xml" if dlna_enabled => respond_xml(
            request,
            200,
            crate::dlna::connection_manager_description().to_string(),
        ),
        route if route.starts_with("/dlna/") => respond_text(request, 503, "DLNA is disabled."),
        "/api/health" => respond_json(
            request,
            200,
            serde_json::json!({
                "status": "ok",
                "readOnly": true,
                "url": public_url(),
                "lanAccess": lan_enabled,
                "dlna": dlna_enabled,
                "ssdpPort": 1900
            })
            .to_string(),
        ),
        "/api/media" if lan_enabled || dlna_enabled => {
            serve_library_file(request, app, library_index, query, FileKind::Media)
        }
        "/api/image" if lan_enabled || dlna_enabled => {
            serve_library_file(request, app, library_index, query, FileKind::Image)
        }
        _ if !lan_enabled => respond_text(request, 503, "LAN web access is disabled."),
        "/api/library" => match read_app_file(app, "video-database.json") {
            Ok(Some(json)) => respond_json(request, 200, json),
            Ok(None) => respond_text(request, 404, "Library database does not exist yet."),
            Err(error) => respond_text(request, 500, &error),
        },
        "/api/settings" => match read_app_file(app, "app-settings.json") {
            Ok(Some(json)) => respond_json(request, 200, public_settings(&json)),
            Ok(None) => respond_json(request, 200, "{}".into()),
            Err(error) => respond_text(request, 500, &error),
        },
        _ => serve_web_asset(request, route),
    }
}

fn read_app_file(app: &tauri::AppHandle, name: &str) -> Result<Option<String>, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not find app data folder: {error}"))?
        .join(name);
    if !path.exists() {
        return Ok(None);
    }
    std::fs::read_to_string(path)
        .map(Some)
        .map_err(|error| format!("Could not read {name}: {error}"))
}

fn public_settings(json: &str) -> String {
    let Ok(mut settings) = serde_json::from_str::<Value>(json) else {
        return "{}".into();
    };
    if let Some(object) = settings.as_object_mut() {
        object.remove("explicit_content_password_hash");
        object.remove("remote_password_hash");
    }
    settings.to_string()
}

#[derive(Default, serde::Deserialize)]
struct RemoteAuthSettings {
    #[serde(default)]
    enable_remote_auth: bool,
    #[serde(default)]
    remote_username: String,
    #[serde(default)]
    remote_password_hash: String,
}

fn is_authorized(request: &Request, app: &tauri::AppHandle) -> bool {
    let Ok(Some(json)) = read_app_file(app, "app-settings.json") else {
        return true;
    };
    let Ok(settings) = serde_json::from_str::<RemoteAuthSettings>(&json) else {
        return true;
    };
    if !settings.enable_remote_auth {
        return true;
    }
    if settings.remote_username.is_empty() || settings.remote_password_hash.is_empty() {
        return false;
    }

    let Some((username, password)) = basic_auth_credentials(request) else {
        return false;
    };
    if username != settings.remote_username {
        return false;
    }

    let Ok(parsed_hash) = PasswordHash::new(&settings.remote_password_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

fn basic_auth_credentials(request: &Request) -> Option<(String, String)> {
    let value = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Authorization"))?
        .value
        .as_str()
        .trim();
    let encoded = value.strip_prefix("Basic ")?;
    let decoded = general_purpose::STANDARD.decode(encoded).ok()?;
    let credentials = String::from_utf8(decoded).ok()?;
    let (username, password) = credentials.split_once(':')?;
    Some((username.to_string(), password.to_string()))
}

#[derive(Clone, Copy)]
enum FileKind {
    Media,
    Image,
}

fn serve_library_file(
    request: Request,
    app: &tauri::AppHandle,
    library_index: &Mutex<LibraryIndex>,
    query: &str,
    kind: FileKind,
) {
    let Some(requested_path) = query_value(query, "path") else {
        respond_text(request, 400, "Missing path.");
        return;
    };
    let allowed = library_index
        .lock()
        .ok()
        .is_some_and(|mut index| index.allows(app, &requested_path, kind));
    if !allowed {
        respond_text(request, 403, "That file is not part of the shared library.");
        return;
    }
    serve_file(request, Path::new(&requested_path), kind);
}

fn query_value(query: &str, wanted_key: &str) -> Option<String> {
    query.split('&').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key == wanted_key).then(|| percent_decode_str(value).decode_utf8_lossy().into_owned())
    })
}

fn request_base_url(request: &Request) -> Option<String> {
    let host = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Host"))?
        .value
        .as_str()
        .trim();
    (!host.is_empty()).then(|| format!("http://{host}"))
}

#[derive(Default)]
struct LibraryIndex {
    modified: Option<SystemTime>,
    media: HashSet<String>,
    images: HashSet<String>,
}

impl LibraryIndex {
    fn allows(&mut self, app: &tauri::AppHandle, requested: &str, kind: FileKind) -> bool {
        let Ok(database_path) = app
            .path()
            .app_data_dir()
            .map(|path| path.join("video-database.json"))
        else {
            return false;
        };
        let Ok(metadata) = std::fs::metadata(&database_path) else {
            return false;
        };
        let modified = metadata.modified().ok();
        if self.modified != modified || self.modified.is_none() {
            let Ok(json) = std::fs::read_to_string(database_path) else {
                return false;
            };
            let Ok(database) = serde_json::from_str::<Value>(&json) else {
                return false;
            };
            self.media.clear();
            self.images.clear();
            if let Some(videos) = database["videos"].as_array() {
                for video in videos {
                    if let Some(path) = video["file_path"].as_str() {
                        self.media.insert(path.to_string());
                    }
                    if let Some(path) = video["artwork_thumbnail"].as_str() {
                        self.images.insert(path.to_string());
                    }
                }
            }
            if let Some(images) = database["actor_thumbnails"].as_object() {
                self.images.extend(
                    images
                        .values()
                        .filter_map(Value::as_str)
                        .map(str::to_string),
                );
            }
            self.modified = modified;
        }

        match kind {
            FileKind::Media => self.media.contains(requested),
            FileKind::Image => self.images.contains(requested),
        }
    }
}

fn serve_file(request: Request, path: &Path, kind: FileKind) {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => {
            respond_text(request, 404, "File not found.");
            return;
        }
    };
    let length = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(_) => {
            respond_text(request, 500, "Could not read file metadata.");
            return;
        }
    };
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    let range = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Range"))
        .and_then(|header| parse_range(header.value.as_str(), length));

    let mut headers = vec![
        header("Content-Type", &mime),
        header("Accept-Ranges", "bytes"),
    ];
    if matches!(kind, FileKind::Media) {
        headers.push(header("transferMode.dlna.org", "Streaming"));
        headers.push(header(
            "contentFeatures.dlna.org",
            "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
        ));
    }
    if let Some((start, end)) = range {
        let response_length = end - start + 1;
        headers.push(header(
            "Content-Range",
            &format!("bytes {start}-{end}/{length}"),
        ));
        headers.push(header("Content-Length", &response_length.to_string()));
        if request.method() == &Method::Head {
            let _ = request.respond(add_headers(Response::empty(StatusCode(206)), headers));
            return;
        }
        if file.seek(SeekFrom::Start(start)).is_err() {
            respond_text(request, 500, "Could not seek in file.");
            return;
        }
        let response = Response::new(
            StatusCode(206),
            headers,
            file.take(response_length),
            None,
            None,
        );
        let _ = request.respond(response);
        return;
    }

    headers.push(header("Content-Length", &length.to_string()));
    if request.method() == &Method::Head {
        let _ = request.respond(add_headers(Response::empty(StatusCode(200)), headers));
    } else {
        let response = Response::new(StatusCode(200), headers, file, None, None);
        let _ = request.respond(response);
    }
}

fn parse_range(value: &str, length: u64) -> Option<(u64, u64)> {
    let range = value.strip_prefix("bytes=")?.split(',').next()?;
    let (start, end) = range.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?.min(length);
        return (suffix > 0).then(|| (length - suffix, length - 1));
    }
    let start = start.parse::<u64>().ok()?;
    if start >= length {
        return None;
    }
    let end = if end.is_empty() {
        length - 1
    } else {
        end.parse::<u64>().ok()?.min(length - 1)
    };
    (start <= end).then_some((start, end))
}

fn serve_web_asset(request: Request, route: &str) {
    let requested = route.trim_start_matches('/');
    let asset_path = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    let asset = WebAssets::get(asset_path).or_else(|| WebAssets::get("index.html"));
    let Some(asset) = asset else {
        respond_text(
            request,
            404,
            "Web application is not embedded in this build.",
        );
        return;
    };
    let mime = mime_guess::from_path(asset_path)
        .first_or_octet_stream()
        .to_string();
    let headers = vec![
        header("Content-Type", &mime),
        header("Cache-Control", "no-cache"),
    ];
    if request.method() == &Method::Head {
        let _ = request.respond(add_headers(Response::empty(StatusCode(200)), headers));
    } else {
        let _ = request.respond(add_headers(
            Response::from_data(asset.data.into_owned()),
            headers,
        ));
    }
}

fn respond_json(request: Request, status: u16, body: String) {
    let response = Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", "application/json; charset=utf-8"))
        .with_header(header("Cache-Control", "no-store"));
    let _ = request.respond(response);
}

fn respond_xml(request: Request, status: u16, body: String) {
    let response = Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", "text/xml; charset=utf-8"))
        .with_header(header("Cache-Control", "no-cache"));
    let _ = request.respond(response);
}

fn respond_text(request: Request, status: u16, body: &str) {
    let response = Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", "text/plain; charset=utf-8"));
    let _ = request.respond(response);
}

fn respond_unauthorized(request: Request) {
    let response = Response::from_string("Authentication is required.")
        .with_status_code(StatusCode(401))
        .with_header(header("Content-Type", "text/plain; charset=utf-8"))
        .with_header(header(
            "WWW-Authenticate",
            "Basic realm=\"ArchiveKong\", charset=\"UTF-8\"",
        ));
    let _ = request.respond(response);
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid HTTP header")
}

fn add_headers<R: Read>(mut response: Response<R>, headers: Vec<Header>) -> Response<R> {
    for header in headers {
        response.add_header(header);
    }
    response
}
