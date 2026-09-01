//! The yt-dlp source: a subprocess fallback.
//!
//! The yt-dlp team repairs YouTube breakage within days, so this
//! source keeps playback alive while a rustypipe fix is pending. It
//! needs the yt-dlp binary on PATH and does nothing without it.
//!
//! yt-dlp writes the audio to stdout. A URL from `--get-url` binds to
//! yt-dlp's own session and answers 403 to another program's fetch,
//! so handing over bytes is the only reliable contract.

use tokio::process::Command;

use super::{AudioSource, BoxFuture};

/// itag 140 is AAC 128kbps in M4A, the format the player decodes.
const FORMAT_SELECTION: &str = "140/bestaudio[ext=m4a]";

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
    ) -> BoxFuture<'a, Result<Vec<u8>, String>> {
        Box::pin(async move {
            let output = Command::new(&self.binary)
                .args(["--quiet", "--no-warnings", "--format", FORMAT_SELECTION])
                .args(["--output", "-"])
                .arg(format!("https://music.youtube.com/watch?v={video_id}"))
                .output()
                .await
                .map_err(|error| format!("could not run yt-dlp: {error}"))?;
            audio_from_output(output.status.success(), output.stdout, &output.stderr)
        })
    }
}

fn audio_from_output(succeeded: bool, stdout: Vec<u8>, stderr: &[u8]) -> Result<Vec<u8>, String> {
    if !succeeded {
        return Err(first_line(stderr).unwrap_or_else(|| "yt-dlp failed".to_string()));
    }
    if stdout.is_empty() {
        return Err("yt-dlp produced no audio".to_string());
    }
    Ok(stdout)
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
    fn stdout_bytes_become_audio() {
        assert_eq!(
            audio_from_output(true, vec![1, 2, 3], b""),
            Ok(vec![1, 2, 3])
        );
    }

    #[test]
    fn a_failure_reports_the_first_stderr_line() {
        let result = audio_from_output(false, vec![], b"ERROR: video unavailable\nmore");
        assert_eq!(result.unwrap_err(), "ERROR: video unavailable");
    }

    #[test]
    fn empty_output_is_an_error() {
        assert!(audio_from_output(true, vec![], b"").is_err());
    }
}
