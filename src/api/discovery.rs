//! TV Music renderer boundary. Only navigation on content cards is actionable.
use crate::core::{
    discovery::{Entry, FeedPage, Shelf, Target},
    model::{ArtistRef, Track, TrackId},
};
use serde_json::Value;
pub fn parse(value: &Value) -> Result<FeedPage, String> {
    let section =
        find(value, "sectionListRenderer").or_else(|| find(value, "sectionListContinuation"));
    if let Some(section) = section {
        let mut shelves = vec![];
        if let Some(contents) = section.get("contents").and_then(Value::as_array) {
            for item in contents {
                if let Some(shelf) = find(item, "shelfRenderer") {
                    let header = shelf
                        .get("header")
                        .or_else(|| shelf.get("headerRenderer"))
                        .unwrap_or(&Value::Null);
                    let title = find(header, "avatarLockupRenderer")
                        .and_then(|h| h.get("title"))
                        .map(text)
                        .or_else(|| {
                            find(header, "shelfHeaderRenderer")
                                .and_then(|h| h.get("title"))
                                .map(text)
                        })
                        .unwrap_or_default();
                    let mut entries = vec![];
                    entries_in(
                        shelf.get("content").unwrap_or(&Value::Null),
                        &mut entries,
                        false,
                    );
                    if !entries.is_empty() {
                        shelves.push(Shelf {
                            title: if title.is_empty() {
                                "More to explore".into()
                            } else {
                                title
                            },
                            entries,
                        });
                    }
                }
            }
        }
        // Some collection pages wrap a grid inside a section, without shelves.
        if !shelves.is_empty() {
            return Ok(FeedPage {
                shelves,
                tracks: vec![],
                continuation: cursor(section),
            });
        }
    }
    for key in [
        "playlistVideoListRenderer",
        "playlistVideoListContinuation",
        "gridRenderer",
        "gridContinuation",
        "playlistPanelRenderer",
        "playlistPanelContinuation",
    ] {
        if let Some(list) = find(value, key) {
            let mut entries = vec![];
            entries_in(list, &mut entries, true);
            let mut tracks = entries
                .into_iter()
                .filter_map(|e| e.track)
                .collect::<Vec<_>>();
            if find(list, "trackTileHeaderRenderer").is_some()
                && let Some(header) = find(value, "entityMetadataRenderer")
            {
                let artist = header
                    .pointer("/bylines/0/lineRenderer/items/0/lineItemRenderer/text")
                    .map(text)
                    .unwrap_or_default();
                for track in &mut tracks {
                    if track.artists.is_empty() && !artist.is_empty() {
                        track.artists.push(ArtistRef::named(&artist));
                    }
                    track.album = header.get("title").map(text);
                }
            }
            if list
                .get("items")
                .or_else(|| list.get("contents"))
                .is_some_and(Value::is_array)
            {
                return Ok(FeedPage {
                    shelves: vec![],
                    tracks,
                    continuation: cursor(list),
                });
            }
        }
    }
    if section.is_some() {
        return Ok(FeedPage::default());
    }
    Err("YouTube couldn’t load this collection. Please try again.".into())
}
/// A generated queue shares the TV `next` response with unrelated suggestions.
/// Only the shelf whose watch endpoints name the requested playlist is its queue.
pub fn parse_radio(value: &Value, playlist: &str) -> Result<FeedPage, String> {
    fn visit(value: &Value, playlist: &str) -> Option<FeedPage> {
        if let Some(shelf) = value.get("shelfRenderer") {
            let content = shelf.get("content")?;
            let mut entries = vec![];
            entries_in(content, &mut entries, true);
            let tracks=entries.into_iter().filter(|entry|matches!(&entry.target,Target::Watch{playlist:Some(id),..} if id==playlist)).filter_map(|entry|entry.track).collect::<Vec<_>>();
            if !tracks.is_empty() {
                return Some(FeedPage {
                    tracks,
                    shelves: vec![],
                    continuation: find(content, "horizontalListRenderer").and_then(cursor),
                });
            }
        }
        match value {
            Value::Object(map) => map
                .iter()
                .filter(|(key, _)| !key.contains("menu") && !key.starts_with("on"))
                .find_map(|(_, v)| visit(v, playlist)),
            Value::Array(values) => values.iter().find_map(|v| visit(v, playlist)),
            _ => None,
        }
    }
    visit(value, playlist)
        .or_else(|| {
            let list = value.pointer("/continuationContents/horizontalListContinuation")?;
            let mut entries = vec![];
            entries_in(list, &mut entries, true);
            let tracks = entries
                .into_iter()
                .filter_map(|e| e.track)
                .collect::<Vec<_>>();
            (!tracks.is_empty()).then(|| FeedPage {
                tracks,
                shelves: vec![],
                continuation: cursor(list),
            })
        })
        .ok_or_else(|| "YouTube couldn’t load this mix. Please try again.".into())
}
fn cursor(value: &Value) -> Option<String> {
    value
        .pointer("/continuations/0/nextContinuationData/continuation")
        .or_else(|| value.pointer("/continuations/0/nextRadioContinuationData/continuation"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
pub(crate) fn find<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(v) = value.get(key) {
        return Some(v);
    }
    match value {
        Value::Object(map) => map
            .iter()
            .filter(|(k, _)| !k.contains("menu") && !k.starts_with("on") && !k.contains("Endpoint"))
            .find_map(|(_, v)| find(v, key)),
        Value::Array(values) => values.iter().find_map(|v| find(v, key)),
        _ => None,
    }
}
fn entries_in(value: &Value, out: &mut Vec<Entry>, songs: bool) {
    if let Some(tile) = value.get("tileRenderer") {
        if let Some(entry) = tile_entry(tile, songs) {
            out.push(entry);
        }
        return;
    }
    if songs {
        for key in [
            "playlistVideoRenderer",
            "playlistPanelVideoRenderer",
            "pivotVideoRenderer",
        ] {
            if let Some(video) = value.get(key) {
                let normalized = serde_json::json!({
                    "onSelectCommand":{"watchEndpoint":{"videoId":video.get("videoId")}},
                    "metadata":{"tileMetadataRenderer":{"title":video.get("title"),"lines":[{"lineRenderer":{"items":[{"lineItemRenderer":{"text":video.get("shortBylineText").or_else(||video.get("longBylineText"))}}]}}]}},
                    "header":{"tileHeaderRenderer":{"thumbnail":video.get("thumbnail"),"thumbnailOverlays":[{"thumbnailOverlayTimeStatusRenderer":{"text":video.get("lengthText")}}]}}
                });
                if let Some(entry) = tile_entry(&normalized, true) {
                    out.push(entry);
                }
                return;
            }
        }
    }
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if !k.contains("menu") && !k.starts_with("on") {
                    entries_in(v, out, songs);
                }
            }
        }
        Value::Array(values) => {
            for v in values {
                entries_in(v, out, songs);
            }
        }
        _ => (),
    }
}
fn optional(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
fn tile_entry(tile: &Value, songs: bool) -> Option<Entry> {
    let command = tile.get("onSelectCommand")?;
    let metadata = tile
        .pointer("/metadata/tileMetadataRenderer")
        .or_else(|| tile.pointer("/header/trackTileHeaderRenderer"))?;
    let title = text(metadata.get("title")?);
    if title.is_empty() {
        return None;
    }
    let subtitle = metadata
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
        .unwrap_or_default();
    let artwork = tile
        .pointer("/header/tileHeaderRenderer/thumbnail/thumbnails")
        .or_else(|| tile.pointer("/header/trackTileHeaderRenderer/thumbnail/thumbnails"))
        .and_then(Value::as_array)
        .and_then(|a| a.last())
        .and_then(|a| optional(a, "url"));
    let mut track = None;
    let target = if let Some(browse) = command.get("browseEndpoint") {
        Target::Browse {
            id: optional(browse, "browseId")?,
            params: optional(browse, "params"),
        }
    } else {
        let watch = command.get("watchEndpoint")?;
        let video = optional(watch, "videoId");
        let playlist = optional(watch, "playlistId");
        if video.is_none() && playlist.is_none() {
            return None;
        }
        // Individual songs often carry an RD radio endpoint as well. A mix card
        // instead has continuePlayback or a non-seed playlist id.
        if let Some(id) = &video
            && (songs
                || (!watch
                    .get("continuePlayback")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    && playlist
                        .as_ref()
                        .is_none_or(|p| p == &format!("RD{id}") || !p.starts_with("RD"))))
        {
            track = Some(Track {
                id: TrackId(id.clone()),
                title: title.clone(),
                artists: if subtitle.is_empty() {
                    vec![]
                } else {
                    vec![ArtistRef::named(subtitle.clone())]
                },
                album: None,
                album_id: None,
                duration: find(
                    tile.get("header").unwrap_or(&Value::Null),
                    "thumbnailOverlayTimeStatusRenderer",
                )
                .and_then(|o| o.get("text"))
                .or_else(|| tile.pointer("/header/trackTileHeaderRenderer/duration"))
                .and_then(|t| super::convert::parse_duration(&text(t))),
                thumbnail_url: artwork.clone(),
                playlist_item_id: None,
            });
        }
        Target::Watch {
            video,
            playlist,
            params: optional(watch, "params"),
        }
    };
    Some(Entry {
        title,
        subtitle,
        artwork,
        target,
        track,
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
                .map(|a| {
                    a.iter()
                        .filter_map(|r| r.get("text").and_then(Value::as_str))
                        .collect()
                })
                .unwrap_or_default()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn tile(command: Value) -> Value {
        json!({"tileRenderer":{"onSelectCommand":command,"metadata":{"tileMetadataRenderer":{"title":{"simpleText":"Title"},"lines":[{"lineRenderer":{"items":[{"lineItemRenderer":{"text":{"simpleText":"Artist"}}}]}}]}},"header":{"tileHeaderRenderer":{"thumbnail":{"thumbnails":[{"url":"https://example.com/art"}]}}}}})
    }
    #[test]
    fn preserves_mixed_shelf_order_and_navigation_without_menu_duplicates() {
        let song = tile(json!({"watchEndpoint":{"videoId":"song","playlistId":"RDsong"}}));
        let mix = tile(json!({"browseEndpoint":{"browseId":"VLRDmix","params":"opaque"}}));
        let radio = tile(
            json!({"watchEndpoint":{"videoId":"seed","playlistId":"RDEMmix","continuePlayback":true}}),
        );
        let section = json!({"contents":[{"shelfRenderer":{"headerRenderer":{"shelfHeaderRenderer":{"avatarLockup":{"avatarLockupRenderer":{"title":{"runs":[{"text":"Mixed for you"}]}}}}},"content":{"horizontalListRenderer":{"items":[song,mix,radio]}},"menuRenderer":{"items":[tile(json!({"watchEndpoint":{"videoId":"duplicate"}}))]}}}],"continuations":[{"nextContinuationData":{"continuation":"section-next"}}]});
        for root in [
            json!({"contents":{"sectionListRenderer":section}}),
            json!({"continuationContents":{"sectionListContinuation":section}}),
        ] {
            let page = parse(&root).unwrap();
            assert_eq!(page.shelves[0].title, "Mixed for you");
            let entries = &page.shelves[0].entries;
            assert_eq!(entries.len(), 3);
            assert_eq!(entries[0].track.as_ref().unwrap().id.0, "song");
            assert!(entries[1].track.is_none());
            assert!(entries[2].track.is_none());
            assert_eq!(
                entries[1].target,
                Target::Browse {
                    id: "VLRDmix".into(),
                    params: Some("opaque".into())
                }
            );
            assert_eq!(page.continuation.as_deref(), Some("section-next"));
        }
    }
    #[test]
    fn collection_tracks_and_unfamiliar_layout() {
        let page=parse(&json!({"contents":{"gridRenderer":{"items":[tile(json!({"watchEndpoint":{"videoId":"song","playlistId":"PLcollection"}}))]}}})).unwrap();
        assert_eq!(page.tracks.len(), 1);
        assert_eq!(page.tracks[0].artist_names(), "Artist");
        assert!(parse(&json!({"error":{}})).is_err());
    }
}

#[cfg(test)]
mod collection_tests {
    use super::*;
    use serde_json::json;
    fn song(id: &str, playlist: &str) -> Value {
        json!({"tileRenderer":{"onSelectCommand":{"watchEndpoint":{"videoId":id,"playlistId":playlist,"continuePlayback":true}},"metadata":{"tileMetadataRenderer":{"title":{"simpleText":"Song"}}}}})
    }
    #[test]
    fn radio_uses_the_requested_queue_not_unrelated_recommendations() {
        let value = json!({"contents":{"singleColumnWatchNextResults":{"pivot":{"sectionListRenderer":{"contents":[
            {"shelfRenderer":{"content":{"horizontalListRenderer":{"items":[song("unrelated","other")]}}}},
            {"shelfRenderer":{"content":{"horizontalListRenderer":{"items":[song("one","RDMIX"),song("two","RDMIX")],"continuations":[{"nextRadioContinuationData":{"continuation":"more"}}]}}}}
        ]}}}}});
        let page = parse_radio(&value, "RDMIX").unwrap();
        assert_eq!(
            page.tracks
                .iter()
                .map(|t| t.id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two"]
        );
        assert_eq!(page.continuation.as_deref(), Some("more"));
        assert!(parse_radio(&value, "unknown").is_err());
    }
    #[test]
    fn parses_playlist_video_rows_including_duration_and_artist() {
        let video = json!({"playlistVideoRenderer":{
            "videoId":"one", "title":{"simpleText":"Song"},
            "shortBylineText":{"runs":[{"text":"Artist"}]},
            "lengthText":{"simpleText":"3:24"},
            "thumbnail":{"thumbnails":[{"url":"https://example.com/art"}]}
        }});
        let value = json!({"contents":{"twoColumnRenderer":{"rightColumn":{"playlistVideoListRenderer":{"contents":[video]}}}}});
        let page = parse(&value).unwrap();
        assert_eq!(page.tracks.len(), 1);
        assert_eq!(page.tracks[0].artist_names(), "Artist");
        assert_eq!(page.tracks[0].duration.unwrap().as_secs(), 204);
    }
}
