use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use serde_json::Value;
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    collections::{hash_map::DefaultHasher, BTreeSet},
    hash::{Hash, Hasher},
    io::ErrorKind,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use tauri::Manager;
use tiny_http::{Header, Request, Response, StatusCode};

use crate::lan_server;

const MULTICAST_ADDRESS: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_PORT: u16 = 1900;
const DEVICE_UUID: &str = "uuid:6f2b7bf5-7f18-4b26-90f5-617263686976";
const DEVICE_TYPE: &str = "urn:schemas-upnp-org:device:MediaServer:1";
const CONTENT_DIRECTORY_TYPE: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";
const CONNECTION_MANAGER_TYPE: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";
const SERVER_HEADER: &str = "Windows/10 UPnP/1.0 ArchiveKong/0.1";
const CACHE_SECONDS: u64 = 1800;
static DLNA_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn start() {
    thread::spawn(|| {
        if let Err(error) = run_ssdp() {
            eprintln!("Could not start ArchiveKong DLNA discovery: {error}");
        }
    });
}

pub fn set_enabled(enabled: bool) {
    DLNA_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    DLNA_ENABLED.load(Ordering::Relaxed)
}

fn run_ssdp() -> Result<(), String> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|error| error.to_string())?;
    socket
        .set_reuse_address(true)
        .map_err(|error| error.to_string())?;
    socket
        .bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, SSDP_PORT).into())
        .map_err(|error| error.to_string())?;
    socket
        .join_multicast_v4(&MULTICAST_ADDRESS, &Ipv4Addr::UNSPECIFIED)
        .map_err(|error| error.to_string())?;
    socket
        .set_multicast_ttl_v4(2)
        .map_err(|error| error.to_string())?;
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| error.to_string())?;
    let socket: UdpSocket = socket.into();

    if is_enabled() {
        send_alive_notifications(&socket);
    }
    let mut last_notification = Instant::now();
    let mut buffer = [0_u8; 4096];

    loop {
        match socket.recv_from(&mut buffer) {
            Ok((length, sender)) => {
                if is_enabled() {
                    let message = String::from_utf8_lossy(&buffer[..length]);
                    respond_to_search(&socket, sender, &message);
                }
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(error) => eprintln!("DLNA discovery receive error: {error}"),
        }

        if is_enabled() && last_notification.elapsed() >= Duration::from_secs(CACHE_SECONDS / 2) {
            send_alive_notifications(&socket);
            last_notification = Instant::now();
        }
    }
}

fn discovery_targets() -> [(&'static str, String); 5] {
    [
        ("upnp:rootdevice", format!("{DEVICE_UUID}::upnp:rootdevice")),
        (DEVICE_UUID, DEVICE_UUID.to_string()),
        (DEVICE_TYPE, format!("{DEVICE_UUID}::{DEVICE_TYPE}")),
        (
            CONTENT_DIRECTORY_TYPE,
            format!("{DEVICE_UUID}::{CONTENT_DIRECTORY_TYPE}"),
        ),
        (
            CONNECTION_MANAGER_TYPE,
            format!("{DEVICE_UUID}::{CONNECTION_MANAGER_TYPE}"),
        ),
    ]
}

fn send_alive_notifications(socket: &UdpSocket) {
    let destination = SocketAddrV4::new(MULTICAST_ADDRESS, SSDP_PORT);
    for (notification_type, usn) in discovery_targets() {
        let message = format!(
            "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nCACHE-CONTROL: max-age={CACHE_SECONDS}\r\nLOCATION: {}/dlna/device.xml\r\nNT: {notification_type}\r\nNTS: ssdp:alive\r\nSERVER: {SERVER_HEADER}\r\nUSN: {usn}\r\n\r\n",
            lan_server::public_url()
        );
        let _ = socket.send_to(message.as_bytes(), destination);
    }
}

fn respond_to_search(socket: &UdpSocket, sender: SocketAddr, message: &str) {
    let normalized = message.replace("\r\n", "\n");
    if !normalized
        .lines()
        .next()
        .is_some_and(|line| line.eq_ignore_ascii_case("M-SEARCH * HTTP/1.1"))
    {
        return;
    }
    let search_target = normalized.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("ST")
            .then(|| value.trim().to_string())
    });
    let Some(search_target) = search_target else {
        return;
    };

    for (target, usn) in discovery_targets() {
        if search_target != "ssdp:all" && !search_target.eq_ignore_ascii_case(target) {
            continue;
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age={CACHE_SECONDS}\r\nEXT:\r\nLOCATION: {}/dlna/device.xml\r\nSERVER: {SERVER_HEADER}\r\nST: {target}\r\nUSN: {usn}\r\n\r\n",
            lan_server::public_url_for_peer(sender)
        );
        let _ = socket.send_to(response.as_bytes(), sender);
    }
}

pub fn device_description() -> String {
    let base = lan_server::public_url();
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<root xmlns="urn:schemas-upnp-org:device-1-0" xmlns:dlna="urn:schemas-dlna-org:device-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <URLBase>{base}/</URLBase>
  <device>
    <deviceType>{DEVICE_TYPE}</deviceType>
    <friendlyName>ArchiveKong</friendlyName>
    <manufacturer>ArchiveKong</manufacturer>
    <manufacturerURL>{base}/</manufacturerURL>
    <modelDescription>ArchiveKong read-only video library</modelDescription>
    <modelName>ArchiveKong Media Server</modelName>
    <modelNumber>0.1</modelNumber>
    <modelURL>{base}/</modelURL>
    <serialNumber>1</serialNumber>
    <UDN>{DEVICE_UUID}</UDN>
    <dlna:X_DLNADOC>DMS-1.50</dlna:X_DLNADOC>
    <serviceList>
      <service>
        <serviceType>{CONTENT_DIRECTORY_TYPE}</serviceType>
        <serviceId>urn:upnp-org:serviceId:ContentDirectory</serviceId>
        <SCPDURL>/dlna/content-directory.xml</SCPDURL>
        <controlURL>/dlna/control/content-directory</controlURL>
        <eventSubURL>/dlna/event/content-directory</eventSubURL>
      </service>
      <service>
        <serviceType>{CONNECTION_MANAGER_TYPE}</serviceType>
        <serviceId>urn:upnp-org:serviceId:ConnectionManager</serviceId>
        <SCPDURL>/dlna/connection-manager.xml</SCPDURL>
        <controlURL>/dlna/control/connection-manager</controlURL>
        <eventSubURL>/dlna/event/connection-manager</eventSubURL>
      </service>
    </serviceList>
    <presentationURL>{base}/</presentationURL>
  </device>
</root>"#
    )
}

pub fn content_directory_description() -> &'static str {
    r#"<?xml version="1.0" encoding="utf-8"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <actionList>
    <action><name>Browse</name><argumentList>
      <argument><name>ObjectID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_ObjectID</relatedStateVariable></argument>
      <argument><name>BrowseFlag</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_BrowseFlag</relatedStateVariable></argument>
      <argument><name>Filter</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Filter</relatedStateVariable></argument>
      <argument><name>StartingIndex</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Index</relatedStateVariable></argument>
      <argument><name>RequestedCount</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>
      <argument><name>SortCriteria</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_SortCriteria</relatedStateVariable></argument>
      <argument><name>Result</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Result</relatedStateVariable></argument>
      <argument><name>NumberReturned</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>
      <argument><name>TotalMatches</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>
      <argument><name>UpdateID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_UpdateID</relatedStateVariable></argument>
    </argumentList></action>
    <action><name>GetSearchCapabilities</name><argumentList><argument><name>SearchCaps</name><direction>out</direction><relatedStateVariable>SearchCapabilities</relatedStateVariable></argument></argumentList></action>
    <action><name>GetSortCapabilities</name><argumentList><argument><name>SortCaps</name><direction>out</direction><relatedStateVariable>SortCapabilities</relatedStateVariable></argument></argumentList></action>
    <action><name>GetSystemUpdateID</name><argumentList><argument><name>Id</name><direction>out</direction><relatedStateVariable>SystemUpdateID</relatedStateVariable></argument></argumentList></action>
  </actionList>
  <serviceStateTable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_ObjectID</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_BrowseFlag</name><dataType>string</dataType><allowedValueList><allowedValue>BrowseMetadata</allowedValue><allowedValue>BrowseDirectChildren</allowedValue></allowedValueList></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_Filter</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_Index</name><dataType>ui4</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_Count</name><dataType>ui4</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_SortCriteria</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_Result</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_UpdateID</name><dataType>ui4</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>SearchCapabilities</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>SortCapabilities</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="yes"><name>SystemUpdateID</name><dataType>ui4</dataType><defaultValue>1</defaultValue></stateVariable>
  </serviceStateTable>
</scpd>"#
}

pub fn connection_manager_description() -> &'static str {
    r#"<?xml version="1.0" encoding="utf-8"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <actionList>
    <action><name>GetProtocolInfo</name><argumentList><argument><name>Source</name><direction>out</direction><relatedStateVariable>SourceProtocolInfo</relatedStateVariable></argument><argument><name>Sink</name><direction>out</direction><relatedStateVariable>SinkProtocolInfo</relatedStateVariable></argument></argumentList></action>
    <action><name>GetCurrentConnectionIDs</name><argumentList><argument><name>ConnectionIDs</name><direction>out</direction><relatedStateVariable>CurrentConnectionIDs</relatedStateVariable></argument></argumentList></action>
    <action><name>GetCurrentConnectionInfo</name><argumentList><argument><name>ConnectionID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_ConnectionID</relatedStateVariable></argument><argument><name>RcsID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_RcsID</relatedStateVariable></argument><argument><name>AVTransportID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_AVTransportID</relatedStateVariable></argument><argument><name>ProtocolInfo</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ProtocolInfo</relatedStateVariable></argument><argument><name>PeerConnectionManager</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ConnectionManager</relatedStateVariable></argument><argument><name>PeerConnectionID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ConnectionID</relatedStateVariable></argument><argument><name>Direction</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Direction</relatedStateVariable></argument><argument><name>Status</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_ConnectionStatus</relatedStateVariable></argument></argumentList></action>
  </actionList>
  <serviceStateTable>
    <stateVariable sendEvents="yes"><name>SourceProtocolInfo</name><dataType>string</dataType></stateVariable><stateVariable sendEvents="yes"><name>SinkProtocolInfo</name><dataType>string</dataType></stateVariable><stateVariable sendEvents="yes"><name>CurrentConnectionIDs</name><dataType>string</dataType></stateVariable>
    <stateVariable sendEvents="no"><name>A_ARG_TYPE_ConnectionStatus</name><dataType>string</dataType></stateVariable><stateVariable sendEvents="no"><name>A_ARG_TYPE_ConnectionManager</name><dataType>string</dataType></stateVariable><stateVariable sendEvents="no"><name>A_ARG_TYPE_Direction</name><dataType>string</dataType></stateVariable><stateVariable sendEvents="no"><name>A_ARG_TYPE_ProtocolInfo</name><dataType>string</dataType></stateVariable><stateVariable sendEvents="no"><name>A_ARG_TYPE_ConnectionID</name><dataType>i4</dataType></stateVariable><stateVariable sendEvents="no"><name>A_ARG_TYPE_AVTransportID</name><dataType>i4</dataType></stateVariable><stateVariable sendEvents="no"><name>A_ARG_TYPE_RcsID</name><dataType>i4</dataType></stateVariable>
  </serviceStateTable>
</scpd>"#
}

pub fn handle_content_directory(mut request: Request, app: &tauri::AppHandle) {
    let action = soap_action(&request);
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body);
    let response = match action.as_deref() {
        Some("Browse") => browse_response(app, &body),
        Some("GetSearchCapabilities") => Ok(action_response(
            CONTENT_DIRECTORY_TYPE,
            "GetSearchCapabilities",
            "<SearchCaps></SearchCaps>",
        )),
        Some("GetSortCapabilities") => Ok(action_response(
            CONTENT_DIRECTORY_TYPE,
            "GetSortCapabilities",
            "<SortCaps>dc:title</SortCaps>",
        )),
        Some("GetSystemUpdateID") => Ok(action_response(
            CONTENT_DIRECTORY_TYPE,
            "GetSystemUpdateID",
            "<Id>1</Id>",
        )),
        _ => Err((401, "Invalid Action")),
    };
    respond_soap(request, response);
}

pub fn handle_connection_manager(request: Request) {
    let response = match soap_action(&request).as_deref() {
        Some("GetProtocolInfo") => Ok(action_response(
            CONNECTION_MANAGER_TYPE,
            "GetProtocolInfo",
            "<Source>http-get:*:video/mp4:*,http-get:*:video/x-matroska:*,http-get:*:video/x-msvideo:*,http-get:*:video/quicktime:*,http-get:*:video/x-ms-wmv:*,http-get:*:video/webm:*</Source><Sink></Sink>",
        )),
        Some("GetCurrentConnectionIDs") => Ok(action_response(
            CONNECTION_MANAGER_TYPE,
            "GetCurrentConnectionIDs",
            "<ConnectionIDs>0</ConnectionIDs>",
        )),
        Some("GetCurrentConnectionInfo") => Ok(action_response(
            CONNECTION_MANAGER_TYPE,
            "GetCurrentConnectionInfo",
            "<RcsID>-1</RcsID><AVTransportID>-1</AVTransportID><ProtocolInfo></ProtocolInfo><PeerConnectionManager></PeerConnectionManager><PeerConnectionID>-1</PeerConnectionID><Direction>Output</Direction><Status>OK</Status>",
        )),
        _ => Err((401, "Invalid Action")),
    };
    respond_soap(request, response);
}

pub fn handle_subscription(request: Request) {
    let response = Response::empty(StatusCode(200))
        .with_header(header("SID", "uuid:archivekong-events"))
        .with_header(header("TIMEOUT", "Second-1800"));
    let _ = request.respond(response);
}

fn soap_action(request: &Request) -> Option<String> {
    let value = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("SOAPACTION"))?
        .value
        .as_str()
        .trim_matches('"');
    Some(value.rsplit('#').next()?.to_string())
}

fn browse_response(app: &tauri::AppHandle, body: &str) -> Result<String, (u16, &'static str)> {
    let object_id = xml_value(body, "ObjectID").unwrap_or_else(|| "0".into());
    let browse_flag =
        xml_value(body, "BrowseFlag").unwrap_or_else(|| "BrowseDirectChildren".into());
    let starting_index = xml_value(body, "StartingIndex")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let requested_count = xml_value(body, "RequestedCount")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let catalog = load_catalog(app).map_err(|_| (501, "Action Failed"))?;
    let browse = catalog
        .browse(&object_id, &browse_flag, starting_index, requested_count)
        .ok_or((701, "No Such Object"))?;
    let fields = format!(
        "<Result>{}</Result><NumberReturned>{}</NumberReturned><TotalMatches>{}</TotalMatches><UpdateID>1</UpdateID>",
        xml_escape(&browse.didl), browse.number_returned, browse.total_matches
    );
    Ok(action_response(CONTENT_DIRECTORY_TYPE, "Browse", &fields))
}

fn action_response(service: &str, action: &str, fields: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action}Response xmlns:u="{service}">{fields}</u:{action}Response></s:Body></s:Envelope>"#
    )
}

fn respond_soap(request: Request, response: Result<String, (u16, &'static str)>) {
    match response {
        Ok(body) => {
            let response = Response::from_string(body)
                .with_status_code(StatusCode(200))
                .with_header(header("Content-Type", "text/xml; charset=\"utf-8\""))
                .with_header(header("EXT", ""));
            let _ = request.respond(response);
        }
        Err((code, description)) => {
            let body = format!(
                r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>{code}</errorCode><errorDescription>{description}</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"#
            );
            let response = Response::from_string(body)
                .with_status_code(StatusCode(500))
                .with_header(header("Content-Type", "text/xml; charset=\"utf-8\""));
            let _ = request.respond(response);
        }
    }
}

fn xml_value(body: &str, name: &str) -> Option<String> {
    let start_tag = format!("<{name}>");
    let end_tag = format!("</{name}>");
    let start = body.find(&start_tag)? + start_tag.len();
    let end = body[start..].find(&end_tag)? + start;
    Some(xml_unescape(&body[start..end]))
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[derive(Clone)]
struct DlnaVideo {
    file_path: String,
    title: String,
    actor: String,
    genre: String,
    year: String,
    artwork: String,
    size: u64,
}

struct Catalog {
    videos: Vec<DlnaVideo>,
}

struct BrowseResult {
    didl: String,
    number_returned: usize,
    total_matches: usize,
}

impl Catalog {
    fn browse(
        &self,
        object_id: &str,
        browse_flag: &str,
        starting_index: usize,
        requested_count: usize,
    ) -> Option<BrowseResult> {
        let metadata = browse_flag == "BrowseMetadata";
        let objects = if metadata {
            vec![self.metadata(object_id)?]
        } else {
            self.children(object_id)?
        };
        let total_matches = objects.len();
        let end = if requested_count == 0 {
            total_matches
        } else {
            starting_index
                .saturating_add(requested_count)
                .min(total_matches)
        };
        let page = if starting_index >= total_matches {
            Vec::new()
        } else {
            objects[starting_index..end].to_vec()
        };
        Some(BrowseResult {
            didl: format!(
                r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" xmlns:dlna="urn:schemas-dlna-org:metadata-1-0/">{}</DIDL-Lite>"#,
                page.join("")
            ),
            number_returned: page.len(),
            total_matches,
        })
    }

    fn metadata(&self, object_id: &str) -> Option<String> {
        match object_id {
            "0" => Some(container_xml("0", "-1", "ArchiveKong", 4)),
            "all" => Some(container_xml("all", "0", "All Videos", self.videos.len())),
            "actors" => Some(container_xml(
                "actors",
                "0",
                "Actors",
                self.group_names(GroupKind::Actor).len(),
            )),
            "genres" => Some(container_xml(
                "genres",
                "0",
                "Genres",
                self.group_names(GroupKind::Genre).len(),
            )),
            "years" => Some(container_xml(
                "years",
                "0",
                "Years",
                self.group_names(GroupKind::Year).len(),
            )),
            _ if object_id.starts_with("actor:") => {
                self.group_metadata(object_id, "actors", GroupKind::Actor)
            }
            _ if object_id.starts_with("genre:") => {
                self.group_metadata(object_id, "genres", GroupKind::Genre)
            }
            _ if object_id.starts_with("year:") => {
                self.group_metadata(object_id, "years", GroupKind::Year)
            }
            _ if object_id.starts_with("video:") => self
                .videos
                .iter()
                .find(|video| video_id(&video.file_path) == object_id)
                .map(|video| item_xml(video, "all")),
            _ => None,
        }
    }

    fn children(&self, object_id: &str) -> Option<Vec<String>> {
        match object_id {
            "0" => Some(vec![
                container_xml("all", "0", "All Videos", self.videos.len()),
                container_xml(
                    "actors",
                    "0",
                    "Actors",
                    self.group_names(GroupKind::Actor).len(),
                ),
                container_xml(
                    "genres",
                    "0",
                    "Genres",
                    self.group_names(GroupKind::Genre).len(),
                ),
                container_xml(
                    "years",
                    "0",
                    "Years",
                    self.group_names(GroupKind::Year).len(),
                ),
            ]),
            "all" => Some(
                self.videos
                    .iter()
                    .map(|video| item_xml(video, "all"))
                    .collect(),
            ),
            "actors" => Some(self.group_containers("actors", "actor", GroupKind::Actor)),
            "genres" => Some(self.group_containers("genres", "genre", GroupKind::Genre)),
            "years" => Some(self.group_containers("years", "year", GroupKind::Year)),
            _ if object_id.starts_with("actor:") => self.group_items(object_id, GroupKind::Actor),
            _ if object_id.starts_with("genre:") => self.group_items(object_id, GroupKind::Genre),
            _ if object_id.starts_with("year:") => self.group_items(object_id, GroupKind::Year),
            _ if object_id.starts_with("video:") => Some(Vec::new()),
            _ => None,
        }
    }

    fn group_names(&self, kind: GroupKind) -> Vec<String> {
        let mut groups = BTreeSet::new();
        for video in &self.videos {
            for value in group_values(video, kind) {
                groups.insert(value);
            }
        }
        groups.into_iter().collect()
    }

    fn group_containers(&self, parent: &str, prefix: &str, kind: GroupKind) -> Vec<String> {
        self.group_names(kind)
            .into_iter()
            .map(|name| {
                let count = self
                    .videos
                    .iter()
                    .filter(|video| group_values(video, kind).contains(&name))
                    .count();
                container_xml(
                    &format!("{prefix}:{}", encode_id(&name)),
                    parent,
                    &name,
                    count,
                )
            })
            .collect()
    }

    fn group_metadata(&self, object_id: &str, parent: &str, kind: GroupKind) -> Option<String> {
        let name = decode_group_id(object_id)?;
        let count = self
            .videos
            .iter()
            .filter(|video| group_values(video, kind).contains(&name))
            .count();
        (count > 0).then(|| container_xml(object_id, parent, &name, count))
    }

    fn group_items(&self, object_id: &str, kind: GroupKind) -> Option<Vec<String>> {
        let name = decode_group_id(object_id)?;
        Some(
            self.videos
                .iter()
                .filter(|video| group_values(video, kind).contains(&name))
                .map(|video| item_xml(video, object_id))
                .collect(),
        )
    }
}

#[derive(Clone, Copy)]
enum GroupKind {
    Actor,
    Genre,
    Year,
}

fn group_values(video: &DlnaVideo, kind: GroupKind) -> Vec<String> {
    let value = match kind {
        GroupKind::Actor => &video.actor,
        GroupKind::Genre => &video.genre,
        GroupKind::Year => &video.year,
    };
    let values = if matches!(kind, GroupKind::Year) {
        vec![value.trim().to_string()]
    } else {
        value
            .split([',', ';', '|'])
            .map(str::trim)
            .map(str::to_string)
            .collect()
    };
    let filtered: Vec<_> = values
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect();
    if filtered.is_empty() {
        vec!["Unknown".into()]
    } else {
        filtered
    }
}

fn load_catalog(app: &tauri::AppHandle) -> Result<Catalog, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let hide_explicit = std::fs::read_to_string(app_data.join("app-settings.json"))
        .ok()
        .and_then(|json| serde_json::from_str::<Value>(&json).ok())
        .and_then(|settings| settings["hide_explicit_content"].as_bool())
        .unwrap_or(false);
    let path = app_data.join("video-database.json");
    let json = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let database: Value = serde_json::from_str(&json).map_err(|error| error.to_string())?;
    let mut videos: Vec<_> = database["videos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|video| {
            if hide_explicit && video["explicit_content"].as_bool().unwrap_or(false) {
                return None;
            }
            Some(DlnaVideo {
                file_path: video["file_path"].as_str()?.to_string(),
                title: video["title"]
                    .as_str()
                    .filter(|title| !title.trim().is_empty())
                    .or_else(|| video["filename"].as_str())?
                    .to_string(),
                actor: video["actor"].as_str().unwrap_or_default().to_string(),
                genre: video["genre"].as_str().unwrap_or_default().to_string(),
                year: video["date"].as_str().unwrap_or_default().to_string(),
                artwork: video["artwork_thumbnail"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                size: video["filesize"].as_u64().unwrap_or_default(),
            })
        })
        .collect();
    videos.sort_by(|first, second| first.title.to_lowercase().cmp(&second.title.to_lowercase()));
    Ok(Catalog { videos })
}

fn container_xml(id: &str, parent_id: &str, title: &str, child_count: usize) -> String {
    format!(
        r#"<container id="{}" parentID="{}" restricted="1" searchable="0" childCount="{child_count}"><dc:title>{}</dc:title><upnp:class>object.container.storageFolder</upnp:class></container>"#,
        xml_escape(id),
        xml_escape(parent_id),
        xml_escape(title)
    )
}

fn item_xml(video: &DlnaVideo, parent_id: &str) -> String {
    let base = lan_server::public_url();
    let media_url = format!(
        "{base}/api/media?path={}",
        utf8_percent_encode(&video.file_path, NON_ALPHANUMERIC)
    );
    let mime = mime_guess::from_path(&video.file_path)
        .first_or_octet_stream()
        .to_string();
    let artwork = if video.artwork.is_empty() {
        String::new()
    } else {
        let artwork_url = format!(
            "{base}/api/image?path={}",
            utf8_percent_encode(&video.artwork, NON_ALPHANUMERIC)
        );
        format!(
            r#"<upnp:albumArtURI dlna:profileID="JPEG_TN">{}</upnp:albumArtURI>"#,
            xml_escape(&artwork_url)
        )
    };
    let creator = if video.actor.trim().is_empty() {
        String::new()
    } else {
        format!("<dc:creator>{}</dc:creator>", xml_escape(&video.actor))
    };
    let genre = if video.genre.trim().is_empty() {
        String::new()
    } else {
        format!("<upnp:genre>{}</upnp:genre>", xml_escape(&video.genre))
    };
    format!(
        r#"<item id="{}" parentID="{}" restricted="1"><dc:title>{}</dc:title>{creator}{genre}{artwork}<upnp:class>object.item.videoItem</upnp:class><res protocolInfo="http-get:*:{mime}:DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000" size="{}">{}</res></item>"#,
        video_id(&video.file_path),
        xml_escape(parent_id),
        xml_escape(&video.title),
        video.size,
        xml_escape(&media_url)
    )
}

fn video_id(path: &str) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("video:{:016x}", hasher.finish())
}

fn encode_id(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

fn decode_group_id(object_id: &str) -> Option<String> {
    let (_, encoded) = object_id.split_once(':')?;
    percent_decode_str(encoded)
        .decode_utf8()
        .ok()
        .map(|value| value.into_owned())
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid DLNA HTTP header")
}
