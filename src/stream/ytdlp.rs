//! The yt-dlp resolver: a subprocess fallback.
//!
//! The yt-dlp team repairs YouTube breakage within days, so this
//! resolver keeps playback alive while a rustypipe fix is pending.
//! It needs the yt-dlp binary on PATH and does nothing without it.

use tokio::process::Command;

use super::{BoxFuture, ResolvedStream, StreamResolver};

/// itag 140 is AAC 128kbps in M4A, the format the player decodes.
const FORMAT_SELECTION: &str = "140/bestaudio[ext=m4a]";

pub struct YtDlpResolver {
    binary: String,
}

impl YtDlpResolver {
    pub fn new() -> Self {
        Self {
            binary: "yt-dlp".to_string(),
        }
    }
}

impl StreamResolver for YtDlpResolver {
    fn name(&self) -> &'static str {
        "yt-dlp"
    }

    fn resolve<'a>(&'a self, video_id: &'a str) -> BoxFuture<'a, Result<ResolvedStream, String>> {
        Box::pin(async move {
            let output = Command::new(&self.binary)
                .args([
                    "--quiet",
                    "--no-warnings",
                    "--format",
                    FORMAT_SELECTION,
                    "--get-url",
                ])
                .arg(format!("https://music.youtube.com/watch?v={video_id}"))
                .output()
                .await
                .map_err(|error| format!("could not run yt-dlp: {error}"))?;
            stream_from_output(output.status.success(), &output.stdout, &output.stderr)
        })
    }
}

fn stream_from_output(
    succeeded: bool,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<ResolvedStream, String> {
    if !succeeded {
        return Err(first_line(stderr).unwrap_or_else(|| "yt-dlp failed".to_string()));
    }
    let url = first_line(stdout).ok_or("yt-dlp printed no URL")?;
    Ok(ResolvedStream {
        url,
        mime: "audio/mp4".to_string(),
    })
}

fn first_line(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let line = text.lines().find(|line| !line.trim().is_empty())?;
    Some(line.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_line_becomes_a_stream() {
        let result = stream_from_output(true, b"https://example.com/a\n", b"");
        assert_eq!(result.unwrap().url, "https://example.com/a");
    }

    #[test]
    fn a_failure_reports_the_first_stderr_line() {
        let result = stream_from_output(false, b"", b"ERROR: video unavailable\nmore");
        assert_eq!(result.unwrap_err(), "ERROR: video unavailable");
    }

    #[test]
    fn empty_output_is_an_error() {
        assert!(stream_from_output(true, b"\n", b"").is_err());
    }
}
