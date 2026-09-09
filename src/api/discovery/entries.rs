use serde_json::Value;

use crate::core::{
    discovery::{Entry, Target},
    model::{ArtistRef, Track, TrackId},
};

use super::renderers::{find, text};

pub(super) fn entries_in(value: &Value, out: &mut Vec<Entry>, songs: bool) {
    if let Some(tile) = value.get("tileRenderer") {
        if let Some(entry) = tile_entry(tile, songs) {
            out.push(entry);
        }
        return;
    }
    if songs && let Some(video) = video_renderer(value) {
        let tile = normalized_video_tile(video);
        if let Some(entry) = tile
            .get("tileRenderer")
            .and_then(|tile| tile_entry(tile, songs))
        {
            out.push(entry);
        }
        return;
    }
    visit_children(value, out, songs);
}

fn tile_entry(value: &Value, songs: bool) -> Option<Entry> {
    let command = value.get("onSelectCommand")?;
    let details = entry_details(value)?;
    let target = entry_target(command)?;
    let track = track_for_tile(value, &details, &target, songs);
    Some(Entry {
        title: details.title,
        subtitle: details.subtitle,
        artwork: details.artwork,
        target,
        track,
    })
}

fn video_renderer(value: &Value) -> Option<&Value> {
    [
        "playlistVideoRenderer",
        "playlistPanelVideoRenderer",
        "pivotVideoRenderer",
    ]
    .into_iter()
    .find_map(|key| value.get(key))
}

fn normalized_video_tile(video: &Value) -> Value {
    serde_json::json!({
        "tileRenderer": {
            "onSelectCommand": {"watchEndpoint": {"videoId": video.get("videoId")}},
            "metadata": {"tileMetadataRenderer": {
                "title": video.get("title"),
                "lines": [{"lineRenderer": {"items": [{"lineItemRenderer": {
                    "text": video.get("shortBylineText").or_else(|| video.get("longBylineText"))
                }}]}}]
            }},
            "header": {"tileHeaderRenderer": {
                "thumbnail": video.get("thumbnail"),
                "thumbnailOverlays": [{"thumbnailOverlayTimeStatusRenderer": {"text": video.get("lengthText")}}]
            }}
        }
    })
}

fn visit_children(value: &Value, out: &mut Vec<Entry>, songs: bool) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if can_visit(key) {
                    entries_in(child, out, songs);
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                entries_in(child, out, songs);
            }
        }
        _ => {}
    }
}

fn can_visit(key: &str) -> bool {
    !key.contains("menu") && !key.starts_with("on")
}

struct EntryDetails {
    title: String,
    subtitle: String,
    artwork: Option<String>,
}

fn entry_details(tile: &Value) -> Option<EntryDetails> {
    let metadata = tile
        .pointer("/metadata/tileMetadataRenderer")
        .or_else(|| tile.pointer("/header/trackTileHeaderRenderer"))?;
    let title = text(metadata.get("title")?);
    (!title.is_empty()).then(|| EntryDetails {
        subtitle: subtitle(metadata),
        artwork: artwork(tile),
        title,
    })
}

fn subtitle(metadata: &Value) -> String {
    metadata
        .pointer("/lines/0/lineRenderer/items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.pointer("/lineItemRenderer/text"))
                .map(text)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

fn artwork(tile: &Value) -> Option<String> {
    tile.pointer("/header/tileHeaderRenderer/thumbnail/thumbnails")
        .or_else(|| tile.pointer("/header/trackTileHeaderRenderer/thumbnail/thumbnails"))
        .and_then(Value::as_array)
        .and_then(|images| images.last())
        .and_then(|image| optional(image, "url"))
}

fn entry_target(command: &Value) -> Option<Target> {
    if let Some(endpoint) = command.get("browseEndpoint") {
        return browse_target(endpoint);
    }
    command.get("watchEndpoint").and_then(watch_target)
}

fn browse_target(endpoint: &Value) -> Option<Target> {
    Some(Target::Browse {
        id: optional(endpoint, "browseId")?,
        params: optional(endpoint, "params"),
    })
}

fn watch_target(endpoint: &Value) -> Option<Target> {
    let video = optional(endpoint, "videoId");
    let playlist = optional(endpoint, "playlistId");
    (video.is_some() || playlist.is_some()).then(|| Target::Watch {
        video,
        playlist,
        params: optional(endpoint, "params"),
    })
}

fn track_for_tile(
    tile: &Value,
    details: &EntryDetails,
    target: &Target,
    songs: bool,
) -> Option<Track> {
    let Target::Watch {
        video: Some(id),
        playlist,
        ..
    } = target
    else {
        return None;
    };
    let endpoint = tile.pointer("/onSelectCommand/watchEndpoint")?;
    track_is_playable(endpoint, id, playlist.as_deref(), songs).then(|| Track {
        id: TrackId(id.clone()),
        title: details.title.clone(),
        artists: artists_for(&details.subtitle),
        album: None,
        album_id: None,
        duration: tile_duration(tile),
        thumbnail_url: details.artwork.clone(),
        playlist_item_id: None,
    })
}

fn track_is_playable(endpoint: &Value, video: &str, playlist: Option<&str>, songs: bool) -> bool {
    songs || (!continues_playback(endpoint) && is_song_radio(video, playlist))
}

fn continues_playback(endpoint: &Value) -> bool {
    endpoint
        .get("continuePlayback")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn is_song_radio(video: &str, playlist: Option<&str>) -> bool {
    playlist.is_none_or(|playlist| playlist == format!("RD{video}") || !playlist.starts_with("RD"))
}

fn artists_for(subtitle: &str) -> Vec<ArtistRef> {
    if subtitle.is_empty() {
        vec![]
    } else {
        vec![ArtistRef::named(subtitle)]
    }
}

fn tile_duration(tile: &Value) -> Option<std::time::Duration> {
    let overlay = find(
        tile.get("header").unwrap_or(&Value::Null),
        "thumbnailOverlayTimeStatusRenderer",
    )
    .and_then(|overlay| overlay.get("text"));
    let duration = overlay.or_else(|| tile.pointer("/header/trackTileHeaderRenderer/duration"))?;
    super::super::convert::parse_duration(&text(duration))
}

fn optional(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn parses_playlist_video_rows_including_duration_and_artist() {
        let video = json!({"playlistVideoRenderer":{"videoId":"one","title":{"simpleText":"Song"},"shortBylineText":{"runs":[{"text":"Artist"}]},"lengthText":{"simpleText":"3:24"},"thumbnail":{"thumbnails":[{"url":"https://example.com/art"}]}}});
        let value = json!({"contents":{"twoColumnRenderer":{"rightColumn":{"playlistVideoListRenderer":{"contents":[video]}}}}});
        let page = super::super::parse(&value).unwrap();
        assert_eq!(page.tracks[0].id.0, "one");
        assert_eq!(page.tracks[0].artist_names(), "Artist");
        assert_eq!(page.tracks[0].duration.unwrap().as_secs(), 204);
    }
    #[test]
    fn malformed_browse_target_does_not_fall_back_to_watch() {
        let value = json!({
            "onSelectCommand": {"browseEndpoint": {}, "watchEndpoint": {"videoId": "video"}},
            "metadata": {"tileMetadataRenderer": {"title": {"simpleText": "Song"}}}
        });
        assert!(super::tile_entry(&value, true).is_none());
    }
}
