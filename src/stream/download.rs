//! Downloads a resolved stream, pushing each chunk to a growing
//! buffer as it arrives.
//!
//! googlevideo rejects a fetch whose user agent does not match the
//! InnerTube client that produced the URL, and it throttles or
//! rejects plain full-file GETs. So the fetch sends the client's user
//! agent and takes googlevideo in ~9MB chunks through the `range` URL
//! parameter, the same way rustypipe-downloader does. Each chunk
//! reaches the buffer as soon as it arrives, so a reader downstream
//! can start decoding long before the whole file is down.

use super::BufferWriter;

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

/// Downloads `stream` into `writer`, one chunk at a time. Does not
/// call `writer.finish`: the caller decides that once the whole
/// resolver attempt succeeds.
pub async fn download_audio(
    http: &reqwest::Client,
    stream: &ResolvedStream,
    writer: &BufferWriter,
) -> Result<(), DownloadError> {
    match googlevideo_size(stream) {
        Some(size) => download_googlevideo(http, stream, size, writer).await,
        None => {
            let bytes = fetch_bytes(http, &stream.url, stream.user_agent.as_deref()).await?;
            writer.push(&bytes);
            Ok(())
        }
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
    writer: &BufferWriter,
) -> Result<(), DownloadError> {
    writer.set_expected_len(size);
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
        writer.push(&chunk);
    }
    Ok(())
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

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::stream::AudioBuffer;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::Duration,
    };
    fn server(response: &'static [u8]) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/audio", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = [0; 2048];
            let _ = socket.read(&mut request);
            socket.write_all(response).unwrap();
        });
        (url, handle)
    }
    #[tokio::test]
    async fn expired_url_and_dropped_connection_can_retry_with_fresh_bytes() {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        for (response, forbidden) in [
            (
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .as_slice(),
                true,
            ),
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\nshort"
                    .as_slice(),
                false,
            ),
        ] {
            let (url, server) = server(response);
            let buffer = AudioBuffer::new(None);
            let writer = buffer.writer();
            let error = download_audio(
                &http,
                &ResolvedStream {
                    url,
                    user_agent: None,
                    size: None,
                },
                &writer,
            )
            .await
            .err()
            .unwrap();
            assert_eq!(error.forbidden, forbidden);
            server.join().unwrap();
            let (url, server) = self::server(
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nfresh",
            );
            assert!(
                download_audio(
                    &http,
                    &ResolvedStream {
                        url,
                        user_agent: None,
                        size: None
                    },
                    &writer
                )
                .await
                .is_ok()
            );
            writer.finish();
            let mut bytes = vec![];
            buffer.reader().read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"fresh");
            server.join().unwrap();
        }
    }
}
