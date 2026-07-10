use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct DesktopRequest {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: &'static str,
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct DesktopResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub status: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body_base64: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}
