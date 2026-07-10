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
const REMOTE_MEDIA_CHUNK_BYTES: u64 = 1024 * 1024;
static LAN_ACCESS_ENABLED: AtomicBool = AtomicBool::new(true);

#[derive(RustEmbed)]
#[folder = "../dist/"]
struct WebAssets;

#[derive(Clone)]
pub struct RelayRequestHeaders {
    pub range: Option<String>,
}

pub struct RelayLocalResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Clone, Copy)]
enum FileKind {
    Media,
    Image,
}

#[derive(Default)]
struct LibraryIndex {
    modified: Option<SystemTime>,
    media: HashSet<String>,
    images: HashSet<String>,
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

pub fn handle_relay_get(
    app: &tauri::AppHandle,
    route_with_query: &str,
    headers: &RelayRequestHeaders,
) -> Result<RelayLocalResponse, String> {
    let library_index = Mutex::new(LibraryIndex::default());
    let (route, query) = route_with_query
        .split_once('?')
        .unwrap_or((route_with_query, ""));

    match route {
        "/api/library" => match read_app_file(app, "video-database.json")? {
            Some(json) => Ok(RelayLocalResponse {
                status: 200,
                headers: json_headers("application/json; charset=utf-8", Some("no-store")),
                body: json.into_bytes(),
            }),
            None => Ok(text_response(404, "Library database does not exist yet.")),
        },
        "/api/settings" => match read_app_file(app, "app-settings.json")? {
            Some(json) => Ok(RelayLocalResponse {
                status: 200,
                headers: json_headers("application/json; charset=utf-8", Some("no-store")),
                body: public_settings(&json).into_bytes(),
            }),
            None => Ok(RelayLocalResponse {
                status: 200,
                headers: json_headers("application/json; charset=utf-8", Some("no-store")),
                body: b"{}".to_vec(),
            }),
        },
        "/api/image" => relay_library_file(app, &library_index, query, FileKind::Image, headers),
        "/api/media" => relay_library_file(app, &library_index, query, FileKind::Media, headers),
        _ => Ok(text_response(404, "Not found.")),
    }
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
        object.remove("relay_device_secret");
    }
    settings.to_string()
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

fn relay_library_file(
    app: &tauri::AppHandle,
    library_index: &Mutex<LibraryIndex>,
    query: &str,
    kind: FileKind,
    headers: &RelayRequestHeaders,
) -> Result<RelayLocalResponse, String> {
    let Some(requested_path) = query_value(query, "path") else {
        return Ok(text_response(400, "Missing path."));
    };
    let allowed = library_index
        .lock()
        .ok()
        .is_some_and(|mut index| index.allows(app, &requested_path, kind));
    if !allowed {
        return Ok(text_response(
            403,
            "That file is not part of the shared library.",
        ));
    }

    relay_file(Path::new(&requested_path), kind, headers)
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
    let head_only = request.method() == &Method::Head;
    match build_file_response(path, kind, None, false, head_only) {
        Ok(FileResponse::Body {
            status,
            headers,
            body,
        }) => {
            let _ = request.respond(Response::new(StatusCode(status), headers, body, None, None));
        }
        Ok(FileResponse::Head { status, headers }) => {
            let _ = request.respond(add_headers(Response::empty(StatusCode(status)), headers));
        }
        Err(error) => respond_text(request, error.status, &error.message),
    }
}

fn relay_file(
    path: &Path,
    kind: FileKind,
    headers: &RelayRequestHeaders,
) -> Result<RelayLocalResponse, String> {
    match build_file_response(path, kind, headers.range.as_deref(), true, false) {
        Ok(FileResponse::Body {
            status,
            headers,
            mut body,
        }) => {
            let mut bytes = Vec::new();
            body.read_to_end(&mut bytes)
                .map_err(|error| format!("Could not read file: {error}"))?;
            Ok(RelayLocalResponse {
                status,
                headers: headers
                    .into_iter()
                    .map(|header| {
                        (
                            header.field.as_str().to_ascii_lowercase().to_string(),
                            header.value.as_str().to_string(),
                        )
                    })
                    .collect(),
                body: bytes,
            })
        }
        Ok(FileResponse::Head { status, headers }) => Ok(RelayLocalResponse {
            status,
            headers: headers
                .into_iter()
                .map(|header| {
                    (
                        header.field.as_str().to_ascii_lowercase().to_string(),
                        header.value.as_str().to_string(),
                    )
                })
                .collect(),
            body: Vec::new(),
        }),
        Err(error) => Ok(text_response(error.status, &error.message)),
    }
}

enum FileResponse {
    Body {
        status: u16,
        headers: Vec<Header>,
        body: Box<dyn Read + Send>,
    },
    Head {
        status: u16,
        headers: Vec<Header>,
    },
}

struct FileResponseError {
    status: u16,
    message: String,
}

fn build_file_response(
    path: &Path,
    kind: FileKind,
    range_header: Option<&str>,
    clamp_media_without_range: bool,
    head_only: bool,
) -> Result<FileResponse, FileResponseError> {
    let mut file = File::open(path).map_err(|_| FileResponseError {
        status: 404,
        message: "File not found.".to_string(),
    })?;
    let length = file
        .metadata()
        .map_err(|_| FileResponseError {
            status: 500,
            message: "Could not read file metadata.".to_string(),
        })?
        .len();
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();

    let requested_range = range_header.and_then(|value| parse_range(value, length));
    let range = requested_range.or_else(|| {
        if clamp_media_without_range && matches!(kind, FileKind::Media) && length > 0 {
            let end = (REMOTE_MEDIA_CHUNK_BYTES - 1).min(length - 1);
            Some((0, end))
        } else {
            None
        }
    });

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
        if clamp_media_without_range {
            headers.push(header("Cache-Control", "private, no-transform"));
        }
    }

    if let Some((start, end)) = range {
        let response_length = end - start + 1;
        headers.push(header(
            "Content-Range",
            &format!("bytes {start}-{end}/{length}"),
        ));
        headers.push(header("Content-Length", &response_length.to_string()));
        file.seek(SeekFrom::Start(start))
            .map_err(|_| FileResponseError {
                status: 500,
                message: "Could not seek in file.".to_string(),
            })?;
        if head_only {
            return Ok(FileResponse::Head {
                status: 206,
                headers,
            });
        }
        return Ok(FileResponse::Body {
            status: 206,
            headers,
            body: Box::new(file.take(response_length)),
        });
    }

    headers.push(header("Content-Length", &length.to_string()));
    if head_only {
        return Ok(FileResponse::Head {
            status: 200,
            headers,
        });
    }
    Ok(FileResponse::Body {
        status: 200,
        headers,
        body: Box::new(file),
    })
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

fn text_response(status: u16, body: &str) -> RelayLocalResponse {
    RelayLocalResponse {
        status,
        headers: vec![(
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        )],
        body: body.as_bytes().to_vec(),
    }
}

fn json_headers(content_type: &str, cache_control: Option<&str>) -> Vec<(String, String)> {
    let mut headers = vec![("content-type".to_string(), content_type.to_string())];
    if let Some(cache_control) = cache_control {
        headers.push(("cache-control".to_string(), cache_control.to_string()));
    }
    headers
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
