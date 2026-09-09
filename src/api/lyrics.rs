//! Public lyrics providers; no account credentials or playback writes.
use crate::core::{
    lyrics::{Lyrics, parse_lrc},
    model::TrackId,
};
use rustypipe::{
    client::RustyPipe,
    error::{Error, ExtractionError},
};
use std::{sync::OnceLock, time::Duration};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    track_name: String,
    artist_name: String,
    duration: f64,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}
fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn matched(record: Record, title: &str, artist: &str, duration: u32) -> Option<Lyrics> {
    if normalized(&record.track_name) != normalized(title)
        || normalized(&record.artist_name) != normalized(artist)
        || !record.duration.is_finite()
        || (record.duration - f64::from(duration)).abs() > 2.0
    {
        return None;
    }
    let timed_lines = parse_lrc(record.synced_lyrics.as_deref().unwrap_or_default());
    let text = if timed_lines.is_empty() {
        record.plain_lyrics.unwrap_or_default()
    } else {
        timed_lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    if text.trim().is_empty() {
        return None;
    }
    Some(Lyrics {
        text,
        source: "Lyrics: LRCLIB".into(),
        timed_lines,
    })
}
async fn lrclib(track: &rustypipe::model::TrackItem) -> Option<Lyrics> {
    let duration = track.duration.filter(|duration| *duration > 0)?;
    let artist = track
        .artists
        .iter()
        .map(|artist| artist.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if artist.is_empty() {
        return None;
    }
    let mut params = vec![
        ("track_name", track.name.clone()),
        ("artist_name", artist.clone()),
        ("duration", duration.to_string()),
    ];
    if let Some(album) = &track.album {
        params.push(("album_name", album.name.clone()));
    }
    static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
    let response = HTTP
        .get_or_init(reqwest::Client::new)
        .get("https://lrclib.net/api/get")
        .header(
            reqwest::header::USER_AGENT,
            "ytamp/0.1.0 (https://github.com/suvojit-0x55aa/ytamp)",
        )
        .query(&params)
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .ok()?;
    // Optional provider: failures (including throttling) never hide YouTube's lyrics.
    if !response.status().is_success() {
        return None;
    }
    let record = serde_json::from_slice(&response.bytes().await.ok()?).ok()?;
    matched(record, &track.name, &artist, duration)
}
fn choose(
    timed: Option<Lyrics>,
    youtube: Result<Option<Lyrics>, String>,
) -> Result<Option<Lyrics>, String> {
    if timed
        .as_ref()
        .is_some_and(|lyrics| !lyrics.timed_lines.is_empty())
    {
        return Ok(timed);
    }
    match youtube {
        Ok(Some(lyrics)) => Ok(Some(lyrics)),
        _ if timed.is_some() => Ok(timed),
        other => other,
    }
}
pub async fn fetch(track: &TrackId) -> Result<Option<Lyrics>, String> {
    static CLIENT: OnceLock<RustyPipe> = OnceLock::new();
    let query = CLIENT.get_or_init(crate::rustypipe_client::new).query();
    let details = query
        .music_details(&track.0)
        .await
        .map_err(|e| e.to_string())?;
    let youtube = async {
        let Some(id) = details.lyrics_id else {
            return Ok(None);
        };
        match query.music_lyrics(id).await {
            Ok(lyrics) if lyrics.body.trim().is_empty() => Ok(None),
            Ok(lyrics) => Ok(Some(Lyrics {
                text: lyrics.body,
                source: lyrics.footer,
                timed_lines: vec![],
            })),
            Err(Error::Extraction(ExtractionError::NotFound { .. })) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    };
    let (timed, plain) = tokio::join!(lrclib(&details.track), youtube);
    choose(timed, plain)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> Record {
        Record {
            track_name: "Song".into(),
            artist_name: "Artist".into(),
            duration: 120.0,
            plain_lyrics: Some("Plain".into()),
            synced_lyrics: Some("[00:01]Timed".into()),
        }
    }
    #[test]
    fn rejects_different_recordings_and_accepts_duration_tolerance() {
        assert!(matched(record(), " Song ", "ARTIST", 122).is_some());
        assert!(matched(record(), "Song", "Artist", 123).is_none());
        assert!(matched(record(), "Song (Live)", "Artist", 120).is_none());
        assert!(matched(record(), "Song", "Other artist", 120).is_none());
    }
    #[test]
    fn provider_failure_preserves_plain_lyrics_and_timing_wins_when_available() {
        let plain = Lyrics {
            text: "YT".into(),
            source: "YT".into(),
            timed_lines: vec![],
        };
        assert_eq!(
            choose(None, Ok(Some(plain.clone()))).unwrap(),
            Some(plain.clone())
        );
        let timed = matched(record(), "Song", "Artist", 120).unwrap();
        assert_eq!(
            choose(Some(timed.clone()), Ok(Some(plain))).unwrap(),
            Some(timed.clone())
        );
        assert_eq!(
            choose(Some(timed.clone()), Err("YT offline".into())).unwrap(),
            Some(timed)
        );
        assert_eq!(choose(None, Ok(None)).unwrap(), None);
        assert!(choose(None, Err("offline".into())).is_err());
    }
}
