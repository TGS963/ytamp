//! YouTube's OAuth TV history renderer. No cookies and no history writes.
use crate::core::{
    listening_history::HistoryPage,
    model::{ArtistRef, Track, TrackId},
};
use serde_json::Value;

pub fn parse(value: &Value) -> Result<HistoryPage, String> {
    let grid = value
        .pointer("/contents/tvBrowseRenderer/content/tvSurfaceContentRenderer/content/gridRenderer")
        .or_else(|| value.pointer("/continuationContents/gridContinuation"))
        .ok_or_else(|| {
            "YouTube returned an unfamiliar history layout. Try refreshing.".to_owned()
        })?;
    let items = grid
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "YouTube did not return a history list.".to_owned())?;
    let tracks = items.iter().filter_map(|item| {
        let tile = item.get("tileRenderer")?;
        let id = tile.pointer("/onSelectCommand/watchEndpoint/videoId")?.as_str()?;
        let title = text(tile.pointer("/metadata/tileMetadataRenderer/title")?);
        if id.is_empty() || title.is_empty() { return None; }
        let artist = tile.pointer("/metadata/tileMetadataRenderer/lines/0/lineRenderer/items/0/lineItemRenderer/text").map(text).unwrap_or_default();
        let thumbnail_url = tile.pointer("/header/tileHeaderRenderer/thumbnail/thumbnails")
            .and_then(Value::as_array).and_then(|images| images.last())
            .and_then(|image| image.get("url")).and_then(Value::as_str).map(str::to_owned);
        let duration = duration(tile.get("header").unwrap_or(&Value::Null));
        Some(Track { id: TrackId(id.to_owned()), title, artists: if artist.is_empty() { vec![] } else { vec![ArtistRef::named(artist)] },
            album: None, album_id: None, duration, thumbnail_url, playlist_item_id: None })
    }).collect();
    let continuation = grid
        .pointer("/continuations/0/nextContinuationData/continuation")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    Ok(HistoryPage {
        tracks,
        continuation,
    })
}
fn text(value: &Value) -> String {
    value
        .get("simpleText")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            value
                .get("runs")
                .and_then(Value::as_array)
                .map(|runs| {
                    runs.iter()
                        .filter_map(|run| run.get("text").and_then(Value::as_str))
                        .collect()
                })
                .unwrap_or_default()
        })
}
fn duration(value: &Value) -> Option<std::time::Duration> {
    if let Some(overlay) = value.get("thumbnailOverlayTimeStatusRenderer") {
        return super::convert::parse_duration(&text(overlay.get("text")?));
    }
    match value {
        Value::Object(map) => map.values().find_map(duration),
        Value::Array(values) => values.iter().find_map(duration),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn parses_tiles_once_without_collecting_menu_duplicates_and_accepts_continuations() {
        let tile = json!({"tileRenderer":{
            "onSelectCommand":{"watchEndpoint":{"videoId":"video"}},
            "onLongPressCommand":{"watchEndpoint":{"videoId":"duplicate"}},
            "metadata":{"tileMetadataRenderer":{"title":{"simpleText":"Example"},"lines":[{"lineRenderer":{"items":[{"lineItemRenderer":{"text":{"runs":[{"text":"Artist"}]}}}]}}]}},
            "header":{"tileHeaderRenderer":{"thumbnail":{"thumbnails":[{"url":"https://example.com/art"}]},"thumbnailOverlays":[{"thumbnailOverlayTimeStatusRenderer":{"text":{"simpleText":"3:45"}}}]}}
        }});
        let grid = json!({"items":[tile,{"tileRenderer":{"contentId":"unplayable"}}],"continuations":[{"nextContinuationData":{"continuation":"next"}}]});
        for value in [
            json!({"contents":{"tvBrowseRenderer":{"content":{"tvSurfaceContentRenderer":{"content":{"gridRenderer":grid}}}}}}),
            json!({"continuationContents":{"gridContinuation":grid}}),
        ] {
            let page = parse(&value).unwrap();
            assert_eq!(page.tracks.len(), 1);
            assert_eq!(page.tracks[0].id.0, "video");
            assert_eq!(page.tracks[0].artists[0].name, "Artist");
            assert_eq!(page.tracks[0].duration.unwrap().as_secs(), 225);
            assert_eq!(page.continuation.as_deref(), Some("next"));
        }
        assert!(parse(&json!({"responseContext":{}})).is_err());
    }
}
