//! OAuth account requests for the official Data API and TV endpoints.

mod endpoints;
mod library;
mod transport;

use tokio::sync::RwLock;

pub struct DataApi {
    http: reqwest::Client,
    token: RwLock<ytmapi_rs::auth::OAuthToken>,
    refresh_client: ytmapi_rs::Client,
    auth_expired: std::sync::atomic::AtomicBool,
}
