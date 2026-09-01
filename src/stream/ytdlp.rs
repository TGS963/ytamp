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

use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};

use super::{AudioSource, BoxFuture, BufferWriter};

/// itag 140 is AAC 128kbps in M4A, the format the player decodes.
const FORMAT_SELECTION: &str = "140/bestaudio[ext=m4a]";

/// The chunk size for reading yt-dlp's stdout.
const CHUNK_BYTES: usize = 64 * 1024;

pub struct YtDlpSource {
    binary: String,
}

impl YtDlpSource {
    pub fn new() -> Self {
        Self {
            binary: "yt-dlp".to_string(),
        }
    }
}

impl AudioSource for YtDlpSource {
    fn name(&self) -> &'static str {
        "yt-dlp"
    }

    fn fetch_audio<'a>(
        &'a self,
        _http: &'a reqwest::Client,
        video_id: &'a str,
        writer: BufferWriter,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let mut child = spawn_yt_dlp(&self.binary, video_id)?;
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
        })
    }
}

/// Starts yt-dlp with its stdout and stderr piped, and `kill_on_drop`
/// so an abandoned attempt (the chain moving to the next source)
/// never leaves the process running.
fn spawn_yt_dlp(binary: &str, video_id: &str) -> Result<Child, String> {
    Command::new(binary)
        .args(["--quiet", "--no-warnings", "--format", FORMAT_SELECTION])
        .args(["--output", "-"])
        .arg(format!("https://music.youtube.com/watch?v={video_id}"))
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
}
