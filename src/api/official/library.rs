use serde_json::Value;

use crate::core::model::{ArtistId, ArtistRef, Playlist, PlaylistId, Track, TrackId};

use super::{DataApi, transport::BASE};

const LIKED_VIDEOS_PLAYLIST: &str = "LL";
const LIKED_PAGE_CAP: usize = 100;
const NO_PAGE_CAP: usize = usize::MAX;

impl DataApi {
    pub async fn playlists(&self) -> Result<Vec<Playlist>, String> {
        let url = format!("{BASE}/playlists?part=snippet,contentDetails&mine=true&maxResults=50");
        let pages = self.get_pages(&url, NO_PAGE_CAP).await?;
        Ok(pages
            .iter()
            .flat_map(items_of)
            .map(parse_playlist)
            .collect())
    }

    pub async fn liked_songs(&self, on_page: impl FnMut(Vec<Track>, bool)) -> Result<(), String> {
        self.tracks_of(LIKED_VIDEOS_PLAYLIST, LIKED_PAGE_CAP, on_page)
            .await
    }

    pub async fn playlist_tracks(
        &self,
        id: &PlaylistId,
        on_page: impl FnMut(Vec<Track>, bool),
    ) -> Result<(), String> {
        self.tracks_of(&id.0, NO_PAGE_CAP, on_page).await
    }

    pub async fn rate_video(&self, id: &TrackId, liked: bool) -> Result<(), String> {
        let url = format!("{BASE}{}", rate_path(id, liked));
        self.post_json(&url, serde_json::json!({})).await?;
        Ok(())
    }

    pub async fn insert_playlist_item(
        &self,
        playlist_id: &PlaylistId,
        track_id: &TrackId,
    ) -> Result<String, String> {
        let url = format!("{BASE}/playlistItems?part=snippet");
        let response = self
            .post_json(&url, playlist_item_body(playlist_id, track_id))
            .await?;
        item_id_of(&response)
    }

    pub async fn delete_playlist_item(&self, item_id: &str) -> Result<(), String> {
        self.delete(&format!("{BASE}/playlistItems?id={item_id}"))
            .await
    }

    pub async fn insert_playlist(&self, title: &str) -> Result<Playlist, String> {
        let response = self
            .post_json(
                &format!("{BASE}/playlists?part=snippet,status"),
                playlist_body(title),
            )
            .await?;
        Ok(parse_playlist(&response))
    }

    pub async fn delete_playlist(&self, id: &PlaylistId) -> Result<(), String> {
        self.delete(&format!("{BASE}/playlists?id={}", id.0)).await
    }

    async fn tracks_of(
        &self,
        playlist_id: &str,
        page_cap: usize,
        mut on_page: impl FnMut(Vec<Track>, bool),
    ) -> Result<(), String> {
        let url =
            format!("{BASE}/playlistItems?part=snippet&playlistId={playlist_id}&maxResults=50");
        let mut page_token = String::new();
        let mut pages_seen = 0;
        loop {
            let page = self.get_json(&paged_url(&url, &page_token)).await?;
            let mut tracks: Vec<Track> = items_of(&page).filter_map(parse_playlist_item).collect();
            self.fill_durations(&mut tracks).await;
            page_token = next_page_token(&page);
            pages_seen += 1;
            let finished = page_token.is_empty() || pages_seen >= page_cap;
            on_page(tracks, finished);
            if finished {
                return Ok(());
            }
        }
    }

    async fn fill_durations(&self, tracks: &mut [Track]) {
        for batch in tracks.chunks_mut(50) {
            let ids: Vec<&str> = batch.iter().map(|track| track.id.0.as_str()).collect();
            let url = format!("{BASE}/videos?part=contentDetails&id={}", ids.join(","));
            match self.get_json(&url).await {
                Ok(page) => apply_durations(batch, &page),
                Err(error) => {
                    log::warn!("duration lookup failed: {error}");
                    return;
                }
            }
        }
    }

    async fn get_pages(&self, url: &str, page_cap: usize) -> Result<Vec<Value>, String> {
        let mut pages = Vec::new();
        let mut page_token = String::new();
        loop {
            let page = self.get_json(&paged_url(url, &page_token)).await?;
            page_token = next_page_token(&page);
            pages.push(page);
            if page_token.is_empty() || pages.len() >= page_cap {
                return Ok(pages);
            }
        }
    }
}

fn paged_url(url: &str, page_token: &str) -> String {
    if page_token.is_empty() {
        return url.to_string();
    }
    format!("{url}&pageToken={page_token}")
}

fn next_page_token(page: &Value) -> String {
    page.get("nextPageToken")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn items_of(page: &Value) -> impl Iterator<Item = &Value> {
    page.get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn rate_path(id: &TrackId, liked: bool) -> String {
    let rating = if liked { "like" } else { "none" };
    format!("/videos/rate?id={}&rating={rating}", id.0)
}

fn playlist_item_body(playlist_id: &PlaylistId, track_id: &TrackId) -> Value {
    serde_json::json!({"snippet":{"playlistId":playlist_id.0,"resourceId":{"kind":"youtube#video","videoId":track_id.0}}})
}

fn playlist_body(title: &str) -> Value {
    serde_json::json!({"snippet":{"title":title},"status":{"privacyStatus":"private"}})
}

fn item_id_of(value: &Value) -> Result<String, String> {
    value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "The write response carried no id.".to_string())
}

fn parse_playlist(item: &Value) -> Playlist {
    Playlist {
        id: PlaylistId(text_of(item, "/id")),
        title: text_of(item, "/snippet/title"),
        track_count: item
            .pointer("/contentDetails/itemCount")
            .and_then(Value::as_u64)
            .map(|n| n as usize),
        thumbnail_url: thumbnail_of(item),
    }
}

fn parse_playlist_item(item: &Value) -> Option<Track> {
    let video_id = item
        .pointer("/snippet/resourceId/videoId")?
        .as_str()?
        .to_string();
    item.pointer("/snippet/videoOwnerChannelTitle")?;
    Some(Track {
        id: TrackId(video_id),
        title: text_of(item, "/snippet/title"),
        artists: vec![video_owner_artist(item)],
        album: None,
        album_id: None,
        duration: None,
        thumbnail_url: thumbnail_of(item),
        playlist_item_id: Some(text_of(item, "/id")),
    })
}

fn video_owner_artist(item: &Value) -> ArtistRef {
    let name = display_channel_title(&text_of(item, "/snippet/videoOwnerChannelTitle"));
    let id = item
        .pointer("/snippet/videoOwnerChannelId")
        .and_then(Value::as_str)
        .map(|id| ArtistId(id.to_string()));
    ArtistRef { name, id }
}

fn display_channel_title(title: &str) -> String {
    title.strip_suffix(" - Topic").unwrap_or(title).to_string()
}

fn apply_durations(tracks: &mut [Track], page: &Value) {
    for item in items_of(page) {
        let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
        let duration = item
            .pointer("/contentDetails/duration")
            .and_then(Value::as_str)
            .and_then(parse_iso8601_duration);
        if let Some(track) = tracks.iter_mut().find(|track| track.id.0 == id) {
            track.duration = duration;
        }
    }
}

fn parse_iso8601_duration(text: &str) -> Option<std::time::Duration> {
    let body = text.strip_prefix("PT")?;
    let mut seconds: u64 = 0;
    let mut number = String::new();
    for character in body.chars() {
        if character.is_ascii_digit() {
            number.push(character);
            continue;
        }
        let value: u64 = number.parse().ok()?;
        number.clear();
        seconds += match character {
            'H' => value * 3600,
            'M' => value * 60,
            'S' => value,
            _ => return None,
        };
    }
    Some(std::time::Duration::from_secs(seconds))
}

fn text_of(item: &Value, pointer: &str) -> String {
    item.pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn thumbnail_of(item: &Value) -> Option<String> {
    for size in ["high", "medium", "default"] {
        let pointer = format!("/snippet/thumbnails/{size}/url");
        if let Some(url) = item.pointer(&pointer).and_then(Value::as_str) {
            return Some(url.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn iso8601_durations_parse() {
        assert_eq!(
            parse_iso8601_duration("PT3M45S"),
            Some(Duration::from_secs(225))
        );
        assert_eq!(
            parse_iso8601_duration("PT1H2M3S"),
            Some(Duration::from_secs(3723))
        );
        assert_eq!(
            parse_iso8601_duration("PT45S"),
            Some(Duration::from_secs(45))
        );
        assert_eq!(parse_iso8601_duration("P1D"), None);
    }

    #[test]
    fn a_topic_channel_title_loses_its_suffix() {
        assert_eq!(display_channel_title("Radiohead - Topic"), "Radiohead");
        assert_eq!(display_channel_title("Radiohead"), "Radiohead");
    }

    #[test]
    fn playlist_items_without_a_channel_are_stubs() {
        let stub =
            serde_json::json!({"snippet":{"resourceId":{"videoId":"abc"},"title":"Deleted video"}});
        assert_eq!(parse_playlist_item(&stub), None);
    }

    #[test]
    fn rate_path_carries_the_video_id_and_rating() {
        let id = TrackId("abc".into());
        assert_eq!(rate_path(&id, true), "/videos/rate?id=abc&rating=like");
        assert_eq!(rate_path(&id, false), "/videos/rate?id=abc&rating=none");
    }

    #[test]
    fn playlist_item_body_names_the_playlist_and_the_video() {
        let body = playlist_item_body(&PlaylistId("PL1".into()), &TrackId("v1".into()));
        assert_eq!(body["snippet"]["playlistId"], "PL1");
        assert_eq!(body["snippet"]["resourceId"]["videoId"], "v1");
        assert_eq!(body["snippet"]["resourceId"]["kind"], "youtube#video");
    }

    #[test]
    fn playlist_body_is_private_with_the_given_title() {
        let body = playlist_body("My Mix");
        assert_eq!(body["snippet"]["title"], "My Mix");
        assert_eq!(body["status"]["privacyStatus"], "private");
    }

    #[test]
    fn item_id_of_reads_the_id_field() {
        let response = serde_json::json!({"id":"item-1"});
        assert_eq!(item_id_of(&response), Ok("item-1".to_string()));
        assert!(item_id_of(&serde_json::json!({})).is_err());
    }

    #[test]
    fn playlists_parse_from_the_official_shape() {
        let item = serde_json::json!({"id":"PL1","snippet":{"title":"Mix","thumbnails":{"high":{"url":"u"}}},"contentDetails":{"itemCount":7}});
        let playlist = parse_playlist(&item);
        assert_eq!(playlist.id.0, "PL1");
        assert_eq!(playlist.title, "Mix");
        assert_eq!(playlist.track_count, Some(7));
        assert_eq!(playlist.thumbnail_url, Some("u".to_string()));
    }
}
