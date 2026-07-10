use std::time::Duration;

#[derive(Clone)]
pub struct RelayLimits {
    pub max_concurrent_requests: usize,
    pub request_timeout: Duration,
    pub max_body_bytes: usize,
}
