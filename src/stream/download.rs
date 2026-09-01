//! Downloads a resolved stream into memory.
//!
//! googlevideo rejects a fetch whose user agent does not match the
//! InnerTube client that produced the URL, and it throttles or
//! rejects plain full-file GETs. So the fetch sends the client's user
//! agent and takes googlevideo in ~9MB chunks through the `range` URL
//! parameter, the same way rustypipe-downloader does.

const DOWNLOAD_CHUNK: u64 = 9_000_000;

/// A direct stream URL with what its download needs.
pub struct ResolvedStream {
    pub url: String,
    /// The user agent of the InnerTube client that produced the URL.
    /// googlevideo rejects a download whose user agent does not match.
    pub user_agent: Option<String>,
    /// The audio size in bytes, when the resolver knows it.
    pub size: Option<u64>,
}

pub struct DownloadError {
    pub message: String,
    /// HTTP 403: the server rejected this URL outright, so the caller
    /// retries with a different client instead of the same URL.
    pub forbidden: bool,
}

pub async fn download_audio(
    http: &reqwest::Client,
    stream: &ResolvedStream,
) -> Result<Vec<u8>, DownloadError> {
    match googlevideo_size(stream) {
        Some(size) => download_googlevideo(http, stream, size).await,
        None => Ok(fetch_bytes(http, &stream.url, stream.user_agent.as_deref())
            .await?
            .to_vec()),
    }
}

/// The known byte size of a googlevideo stream: the resolver's answer,
/// or the URL's own clen parameter.
fn googlevideo_size(stream: &ResolvedStream) -> Option<u64> {
    if !stream.url.contains(".googlevideo.com/videoplayback") {
        return None;
    }
    stream.size.or_else(|| clen_parameter(&stream.url))
}

fn clen_parameter(url: &str) -> Option<u64> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let clen = parsed
        .query_pairs()
        .find(|(key, _)| key == "clen")?
        .1
        .into_owned();
    clen.parse().ok()
}

async fn download_googlevideo(
    http: &reqwest::Client,
    stream: &ResolvedStream,
    size: u64,
) -> Result<Vec<u8>, DownloadError> {
    let mut bytes = Vec::with_capacity(size as usize);
    let mut offset = 0;
    while offset < size {
        let end = (offset + DOWNLOAD_CHUNK - 1).min(size - 1);
        let url = format!("{}&range={offset}-{end}", stream.url);
        let chunk = fetch_bytes(http, &url, stream.user_agent.as_deref()).await?;
        if chunk.is_empty() {
            return Err(DownloadError {
                message: "The audio download returned an empty chunk".to_string(),
                forbidden: false,
            });
        }
        offset += chunk.len() as u64;
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn fetch_bytes(
    http: &reqwest::Client,
    url: &str,
    user_agent: Option<&str>,
) -> Result<bytes::Bytes, DownloadError> {
    let mut request = http.get(url);
    if let Some(user_agent) = user_agent {
        request = request.header(reqwest::header::USER_AGENT, user_agent);
    }
    let response = request
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(download_error)?;
    response.bytes().await.map_err(download_error)
}

fn download_error(error: reqwest::Error) -> DownloadError {
    DownloadError {
        message: format!("The audio download failed: {error}"),
        forbidden: error.status() == Some(reqwest::StatusCode::FORBIDDEN),
    }
}
