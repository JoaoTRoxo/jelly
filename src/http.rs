//! What the Jellyfin and Seerr clients share for HTTP: an agent with Jelly's
//! user agent and timeouts, and limits on how much a reply may be.

use serde::de::DeserializeOwned;
use std::time::Duration;

pub type Response = ureq::http::Response<ureq::Body>;

/// The most a JSON reply is read of (a whole library's listing, with every
/// overview, can be tens of MB)
const JSON_LIMIT: u64 = 256 * 1024 * 1024;
/// The most an image is read of
const IMAGE_LIMIT: u64 = 32 * 1024 * 1024;

/// An agent that sends Jelly's user agent (proxies in front of servers may
/// turn away unknown ones) and gives up on a server that stops answering.
/// Error statuses come back as replies, so that their bodies can say what
/// went wrong. `limit` bounds each whole call, if given.
pub fn agent(limit: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(format!(
            "{}/{}",
            crate::api::CLIENT_NAME,
            crate::api::CLIENT_VERSION
        ))
        .timeout_global(limit)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(120)))
        .http_status_as_error(false)
        .build()
        .into()
}

pub fn read_json<T: DeserializeOwned>(mut response: Response) -> Result<T, ureq::Error> {
    response
        .body_mut()
        .with_config()
        .limit(JSON_LIMIT)
        .read_json()
}

pub fn read_image(mut response: Response) -> Result<Vec<u8>, ureq::Error> {
    response
        .body_mut()
        .with_config()
        .limit(IMAGE_LIMIT)
        .read_to_vec()
}

/// What went wrong reaching a server, without its (long) URL: "Connection
/// refused (os error 111)"
pub fn problem(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Io(e) => e.to_string(),
        e => e.to_string(),
    }
}
