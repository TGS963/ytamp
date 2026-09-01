//! The yt-dlp source: a subprocess fallback.
//!
//! The yt-dlp team repairs YouTube breakage within days, so this
//! source keeps playback alive while a rustypipe fix is pending. It
//! needs the yt-dlp binary on PATH and does nothing without it.
//!
//! yt-dlp writes the audio to stdout. A URL from `--get-url` binds to
//! yt-dlp's own session and answers 403 to another program's fetch,
//! so handing over bytes is the only reliable contract. This source
//! reads those bytes in chunks and pushes each one, so a decoder
//! reading the buffer can start before yt-dlp finishes.
//!
//! Two extraction paths run in order. The fast path hands yt-dlp a
//! cached visitor data token, so it skips its own webpage fetch. The
//! slow path is the plain, always-available extraction. A fast
//! failure before the first byte falls through to the slow path; a
//! fast failure after the first byte ends the attempt, the same as a
//! slow failure does.

use std::process::Stdio;
use std::sync::atomic::{AtomicU8, Ordering};

use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};

use super::visitor_data::VisitorData;
use super::{AudioSource, BoxFuture, BufferWriter};

/// itag 140 is AAC 128kbps in M4A, the format the player decodes.
const FORMAT_SELECTION: &str = "140/bestaudio[ext=m4a]";

/// yt-dlp downloads the HLS and DASH manifests before it picks a
/// format. Our format is a plain progressive stream, so the manifests
/// only cost time: about 700 ms of the 2300 ms to the first byte.
const BASE_EXTRACTOR_ARGS: &str = "youtube:skip=hls,dash,translated_subs";

/// Extra extractor arguments for the fast path. With a visitor data
/// token in hand, yt-dlp skips the webpage, the player config, and
/// the initial data fetch, and uses the token in their place.
const FAST_EXTRACTOR_ARGS: &str =
    ";player_client=visionos;player_skip=webpage,configs,initial_data;visitor_data=";

/// The chunk size for reading yt-dlp's stdout.
const CHUNK_BYTES: usize = 64 * 1024;

/// Fast attempts that failed before the first byte, in a row. At the
/// limit the fast path stays off for the session, so a blocked fast
/// path never doubles the time to the first byte on every track.
const FAST_PATH_FAILURE_LIMIT: u8 = 2;

pub struct YtDlpSource {
    binary: String,
    visitor_data: VisitorData,
    fast_path_failures: AtomicU8,
}

impl YtDlpSource {
    pub fn new() -> Self {
        Self {
            binary: "yt-dlp".to_string(),
            visitor_data: VisitorData::new(),
            fast_path_failures: AtomicU8::new(0),
        }
    }

    fn fast_path_open(&self) -> bool {
        self.fast_path_failures.load(Ordering::SeqCst) < FAST_PATH_FAILURE_LIMIT
    }

    fn record_fast_path_failure(&self) {
        let failures = self.fast_path_failures.fetch_add(1, Ordering::SeqCst) + 1;
        if failures == FAST_PATH_FAILURE_LIMIT {
            log::warn!("the fast yt-dlp path failed {failures} times; using the slow path");
        }
    }

    fn record_fast_path_success(&self) {
        self.fast_path_failures.store(0, Ordering::SeqCst);
    }
}

impl AudioSource for YtDlpSource {
    fn name(&self) -> &'static str {
        "yt-dlp"
    }

    fn warm_up<'a>(&'a self, http: &'a reqwest::Client) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.visitor_data.get(http).await;
        })
    }

    fn fetch_audio<'a>(
        &'a self,
        http: &'a reqwest::Client,
        video_id: &'a str,
        writer: BufferWriter,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let token = match self.fast_path_open() {
                true => self.visitor_data.get(http).await,
                false => None,
            };
            match token {
                Some(token) => self.fetch_with_fast_path(video_id, &token, writer).await,
                None => {
                    self.run_attempt(video_id, &extractor_args(None), writer)
                        .await
                }
            }
        })
    }
}

impl YtDlpSource {
    /// Tries the fast path first. A failure with bytes already
    /// delivered ends the attempt, the same as the slow path alone
    /// would. A failure before the first byte drops the cached token
    /// and retries with the slow path, on the same writer.
    async fn fetch_with_fast_path(
        &self,
        video_id: &str,
        token: &str,
        writer: BufferWriter,
    ) -> Result<(), String> {
        let fast_args = extractor_args(Some(token));
        let message = match self.run_attempt(video_id, &fast_args, writer.share()).await {
            Ok(()) => {
                self.record_fast_path_success();
                return Ok(());
            }
            Err(message) if writer.delivered_any() => return Err(message),
            Err(message) => message,
        };
        log::info!("fast yt-dlp attempt failed before the first byte: {message}");
        self.record_fast_path_failure();
        self.visitor_data.invalidate().await;
        self.run_attempt(video_id, &extractor_args(None), writer)
            .await
    }

    /// Runs one full yt-dlp attempt: spawn, stream stdout into
    /// `writer`, and settle the writer's final state from the exit
    /// status and any stderr text.
    async fn run_attempt(
        &self,
        video_id: &str,
        extractor_args: &str,
        writer: BufferWriter,
    ) -> Result<(), String> {
        let args = command_args(video_id, extractor_args);
        let mut child = spawn_yt_dlp(&self.binary, &args)?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let stderr_task = tokio::spawn(collect_stderr(stderr));
        let read_result = push_chunks(stdout, &writer).await;
        let status = child
            .wait()
            .await
            .map_err(|error| format!("yt-dlp did not exit cleanly: {error}"))?;
        let stderr_bytes = stderr_task.await.unwrap_or_default();
        report_outcome(status.success(), read_result, writer, &stderr_bytes)
    }
}

/// The extractor arguments for one attempt. With a visitor data
/// token, the fast path arguments are appended so yt-dlp skips its
/// own webpage fetch. Without one, only the base arguments apply.
fn extractor_args(visitor_data: Option<&str>) -> String {
    match visitor_data {
        Some(token) => format!("{BASE_EXTRACTOR_ARGS}{FAST_EXTRACTOR_ARGS}{token}"),
        None => BASE_EXTRACTOR_ARGS.to_string(),
    }
}

/// The full yt-dlp command line, `--quiet` and its friends first,
/// the video URL last, ready for `Command::args`.
fn command_args(video_id: &str, extractor_args: &str) -> Vec<String> {
    [
        "--quiet",
        "--no-warnings",
        "--no-playlist",
        "--format",
        FORMAT_SELECTION,
        "--extractor-args",
        extractor_args,
        "--output",
        "-",
    ]
    .into_iter()
    .map(str::to_string)
    .chain(std::iter::once(format!(
        "https://music.youtube.com/watch?v={video_id}"
    )))
    .collect()
}

/// Starts yt-dlp with its stdout and stderr piped, and `kill_on_drop`
/// so an abandoned attempt (the chain moving to the next source)
/// never leaves the process running.
fn spawn_yt_dlp(binary: &str, args: &[String]) -> Result<Child, String> {
    Command::new(binary)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("could not run yt-dlp: {error}"))
}

/// Reads `stdout` in fixed-size chunks and pushes each one to
/// `writer`, until the process closes the stream.
async fn push_chunks(mut stdout: ChildStdout, writer: &BufferWriter) -> Result<(), String> {
    let mut chunk = vec![0u8; CHUNK_BYTES];
    loop {
        let read = stdout
            .read(&mut chunk)
            .await
            .map_err(|error| format!("reading yt-dlp output failed: {error}"))?;
        if read == 0 {
            return Ok(());
        }
        writer.push(&chunk[..read]);
    }
}

/// Reads all of `stderr` into memory. yt-dlp's error text is a few
/// lines at most, so this never competes for memory with the audio.
async fn collect_stderr(mut stderr: ChildStderr) -> Vec<u8> {
    let mut collected = Vec::new();
    let _ = stderr.read_to_end(&mut collected).await;
    collected
}

/// Turns the process exit status and the read outcome into the
/// trait's contract: `finish` the writer and return `Ok` on a clean
/// run, otherwise fail the writer only if bytes already reached it,
/// and report the failure either way.
fn report_outcome(
    exited_cleanly: bool,
    read_result: Result<(), String>,
    writer: BufferWriter,
    stderr: &[u8],
) -> Result<(), String> {
    match read_result {
        Ok(()) if exited_cleanly => {
            writer.finish();
            Ok(())
        }
        Ok(()) => conclude_failure(failure_message(stderr), writer),
        Err(message) => conclude_failure(message, writer),
    }
}

fn failure_message(stderr: &[u8]) -> String {
    first_line(stderr).unwrap_or_else(|| "yt-dlp failed".to_string())
}

fn conclude_failure(message: String, writer: BufferWriter) -> Result<(), String> {
    if writer.delivered_any() {
        writer.fail(message.clone());
    }
    Err(message)
}

fn first_line(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let line = text.lines().find(|line| !line.trim().is_empty())?;
    Some(line.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::AudioBuffer;

    #[test]
    fn a_failure_reports_the_first_stderr_line() {
        let result = report_outcome(
            false,
            Ok(()),
            AudioBuffer::new(None).writer(),
            b"ERROR: video unavailable\nmore",
        );
        assert_eq!(result.unwrap_err(), "ERROR: video unavailable");
    }

    #[test]
    fn a_clean_exit_finishes_the_writer() {
        let buffer = AudioBuffer::new(None);
        let writer = buffer.writer();
        report_outcome(true, Ok(()), writer, b"").expect("a clean exit is ok");
        assert_eq!(buffer.status(), crate::stream::BufferStatus::Complete);
    }

    #[test]
    fn a_failure_after_delivery_fails_the_writer() {
        let buffer = AudioBuffer::new(None);
        let writer = buffer.writer();
        writer.push(b"partial");
        let result = report_outcome(false, Ok(()), writer, b"ERROR: cut off");
        assert_eq!(result.unwrap_err(), "ERROR: cut off");
        assert_eq!(
            buffer.status(),
            crate::stream::BufferStatus::Failed("ERROR: cut off".to_string())
        );
    }

    #[test]
    fn a_read_error_is_reported_over_a_missing_stderr_line() {
        let result = report_outcome(
            true,
            Err("reading yt-dlp output failed: broken pipe".to_string()),
            AudioBuffer::new(None).writer(),
            b"",
        );
        assert_eq!(result.unwrap_err(), "reading yt-dlp output failed: broken pipe");
    }

    #[test]
    fn empty_stderr_falls_back_to_a_generic_message() {
        let result = report_outcome(false, Ok(()), AudioBuffer::new(None).writer(), b"");
        assert_eq!(result.unwrap_err(), "yt-dlp failed");
    }

    #[test]
    fn extractor_args_omits_fast_path_fields_without_a_token() {
        assert_eq!(extractor_args(None), "youtube:skip=hls,dash,translated_subs");
    }

    #[test]
    fn extractor_args_appends_the_fast_path_fields_with_a_token() {
        assert_eq!(
            extractor_args(Some("TOKEN%3D%3D")),
            "youtube:skip=hls,dash,translated_subs;player_client=visionos;\
             player_skip=webpage,configs,initial_data;visitor_data=TOKEN%3D%3D"
        );
    }

    #[test]
    fn command_args_places_the_url_last() {
        let args = command_args("abc123", "youtube:skip=hls");
        assert_eq!(
            args,
            vec![
                "--quiet",
                "--no-warnings",
                "--no-playlist",
                "--format",
                "140/bestaudio[ext=m4a]",
                "--extractor-args",
                "youtube:skip=hls",
                "--output",
                "-",
                "https://music.youtube.com/watch?v=abc123",
            ]
        );
    }
}
