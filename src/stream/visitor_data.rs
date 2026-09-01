//! Visitor data for the fast yt-dlp extraction path.
//!
//! YouTube's home page embeds a `VISITOR_DATA` token. yt-dlp accepts
//! this token as an extractor argument and skips its own webpage
//! fetch when it has one, which saves most of the time to first
//! byte. This module fetches the token once, keeps it in memory, and
//! drops it when a fast attempt fails, so the next track tries a
//! fresh fetch instead of repeating a bad value.

use tokio::sync::Mutex;

/// The page a fresh visitor data token comes from.
const HOME_PAGE_URL: &str = "https://www.youtube.com/";

/// A desktop browser user agent. YouTube serves a different, script-
/// heavy page to an unrecognized client, and that page may not embed
/// `VISITOR_DATA` at all.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// A cached visitor data token, fetched once and reused until
/// `VisitorData::invalidate` drops it.
pub struct VisitorData {
    cached: Mutex<Option<String>>,
}

impl VisitorData {
    pub fn new() -> Self {
        Self {
            cached: Mutex::new(None),
        }
    }

    /// The cached token, fetching it first when the cache is empty.
    /// `None` when the fetch fails; a warning is logged in that case.
    pub async fn get(&self, http: &reqwest::Client) -> Option<String> {
        let mut cached = self.cached.lock().await;
        if cached.is_none() {
            *cached = fetch(http).await;
        }
        cached.clone()
    }

    /// Drops the cached token, so the next `get` call fetches a fresh
    /// one. Call this when a fast attempt using the cached value
    /// fails before its first byte.
    pub async fn invalidate(&self) {
        *self.cached.lock().await = None;
    }
}

/// Downloads the home page and extracts its visitor data token.
/// `None` on a request failure, a non-success status, or a page with
/// no token, each logged as a warning.
async fn fetch(http: &reqwest::Client) -> Option<String> {
    let response = http
        .get(HOME_PAGE_URL)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .inspect_err(|error| log::warn!("visitor data fetch failed: {error}"))
        .ok()?;
    let body = response
        .error_for_status()
        .inspect_err(|error| log::warn!("visitor data fetch failed: {error}"))
        .ok()?
        .text()
        .await
        .inspect_err(|error| log::warn!("visitor data fetch failed: {error}"))
        .ok()?;
    let token = parse_visitor_data(&body);
    if token.is_none() {
        log::warn!("visitor data not found on the home page");
    }
    token
}

/// Pulls the `VISITOR_DATA` value out of a YouTube page body. The
/// value sits in a `"VISITOR_DATA":"..."` field, URL-safe base64
/// with `%3D` padding, and is returned exactly as it appears, since
/// yt-dlp expects it verbatim.
fn parse_visitor_data(html: &str) -> Option<String> {
    const MARKER: &str = "\"VISITOR_DATA\":\"";
    let start = html.find(MARKER)? + MARKER.len();
    let rest = &html[start..];
    let end = rest.find('"')?;
    let value = &rest[..end];
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_visitor_data_finds_a_present_token() {
        let html = r#"foo "VISITOR_DATA":"CgtabmFtZQ%3D%3D" bar"#;
        assert_eq!(
            parse_visitor_data(html),
            Some("CgtabmFtZQ%3D%3D".to_string())
        );
    }

    #[test]
    fn parse_visitor_data_reports_none_when_absent() {
        assert_eq!(parse_visitor_data("no token on this page"), None);
    }

    #[test]
    fn parse_visitor_data_reports_none_for_an_empty_value() {
        assert_eq!(parse_visitor_data(r#""VISITOR_DATA":""#), None);
    }

    #[test]
    fn parse_visitor_data_reports_none_for_an_unclosed_field() {
        assert_eq!(parse_visitor_data(r#""VISITOR_DATA":"unterminated"#), None);
    }
}
