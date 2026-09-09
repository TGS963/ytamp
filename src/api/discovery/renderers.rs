use serde_json::Value;

use crate::core::{
    discovery::{FeedPage, Shelf, Target},
    model::ArtistRef,
};

use super::entries::entries_in;

pub fn parse(value: &Value) -> Result<FeedPage, String> {
    let section = section_renderer(value);
    if let Some(page) = section_page(section) {
        return Ok(page);
    }
    collection_page(value, section.is_some())
}

pub fn parse_radio(value: &Value, playlist: &str) -> Result<FeedPage, String> {
    radio_shelf(value, playlist)
        .or_else(|| radio_continuation(value))
        .ok_or_else(|| "YouTube couldn’t load this mix. Please try again.".into())
}

fn section_renderer(value: &Value) -> Option<&Value> {
    find(value, "sectionListRenderer").or_else(|| find(value, "sectionListContinuation"))
}

fn section_page(section: Option<&Value>) -> Option<FeedPage> {
    let section = section?;
    let shelves = section
        .get("contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(shelf_from_item)
        .collect::<Vec<_>>();
    (!shelves.is_empty()).then(|| FeedPage {
        shelves,
        tracks: vec![],
        continuation: cursor(section),
    })
}

fn shelf_from_item(item: &Value) -> Option<Shelf> {
    let shelf = find(item, "shelfRenderer")?;
    let entries = entries_for(shelf.get("content")?, false);
    (!entries.is_empty()).then(|| Shelf {
        title: shelf_title(shelf),
        entries,
    })
}

fn shelf_title(shelf: &Value) -> String {
    let header = shelf
        .get("header")
        .or_else(|| shelf.get("headerRenderer"))
        .unwrap_or(&Value::Null);
    let title = find(header, "avatarLockupRenderer")
        .and_then(|header| header.get("title"))
        .map(text)
        .or_else(|| {
            find(header, "shelfHeaderRenderer")
                .and_then(|header| header.get("title"))
                .map(text)
        })
        .unwrap_or_default();
    if title.is_empty() {
        "More to explore".into()
    } else {
        title
    }
}

fn collection_page(value: &Value, has_section: bool) -> Result<FeedPage, String> {
    for key in collection_renderer_names() {
        if let Some(page) = collection_page_for(value, key) {
            return Ok(page);
        }
    }
    if has_section {
        Ok(FeedPage::default())
    } else {
        Err("YouTube couldn’t load this collection. Please try again.".into())
    }
}

fn collection_renderer_names() -> [&'static str; 6] {
    [
        "playlistVideoListRenderer",
        "playlistVideoListContinuation",
        "gridRenderer",
        "gridContinuation",
        "playlistPanelRenderer",
        "playlistPanelContinuation",
    ]
}

fn collection_page_for(value: &Value, key: &str) -> Option<FeedPage> {
    let list = find(value, key)?;
    let has_items = list
        .get("items")
        .or_else(|| list.get("contents"))
        .is_some_and(Value::is_array);
    if !has_items {
        return None;
    }
    let mut tracks: Vec<crate::core::model::Track> = entries_for(list, true)
        .into_iter()
        .filter_map(|entry| entry.track)
        .collect();
    add_collection_metadata(value, list, &mut tracks);
    Some(FeedPage {
        shelves: vec![],
        tracks,
        continuation: cursor(list),
    })
}

fn add_collection_metadata(value: &Value, list: &Value, tracks: &mut [crate::core::model::Track]) {
    if find(list, "trackTileHeaderRenderer").is_none() {
        return;
    }
    let Some(header) = find(value, "entityMetadataRenderer") else {
        return;
    };
    let artist = header
        .pointer("/bylines/0/lineRenderer/items/0/lineItemRenderer/text")
        .map(text)
        .unwrap_or_default();
    let album = header.get("title").map(text);
    for track in tracks {
        if track.artists.is_empty() && !artist.is_empty() {
            track.artists.push(ArtistRef::named(&artist));
        }
        track.album = album.clone();
    }
}

fn radio_shelf(value: &Value, playlist: &str) -> Option<FeedPage> {
    if let Some(shelf) = value.get("shelfRenderer") {
        let content = shelf.get("content")?;
        if let Some(page) = radio_page(content, playlist) {
            return Some(page);
        }
    }
    match value {
        Value::Object(map) => map
            .iter()
            .filter(|(key, _)| !key.contains("menu") && !key.starts_with("on"))
            .find_map(|(_, child)| radio_shelf(child, playlist)),
        Value::Array(values) => values.iter().find_map(|child| radio_shelf(child, playlist)),
        _ => None,
    }
}

fn radio_page(content: &Value, playlist: &str) -> Option<FeedPage> {
    let tracks = entries_for(content, true)
        .into_iter()
        .filter(|entry| matches!(&entry.target, Target::Watch { playlist: Some(id), .. } if id == playlist))
        .filter_map(|entry| entry.track)
        .collect::<Vec<_>>();
    (!tracks.is_empty()).then(|| FeedPage {
        tracks,
        shelves: vec![],
        continuation: find(content, "horizontalListRenderer").and_then(cursor),
    })
}

fn radio_continuation(value: &Value) -> Option<FeedPage> {
    let list = value.pointer("/continuationContents/horizontalListContinuation")?;
    let tracks = entries_for(list, true)
        .into_iter()
        .filter_map(|entry| entry.track)
        .collect::<Vec<_>>();
    (!tracks.is_empty()).then(|| FeedPage {
        tracks,
        shelves: vec![],
        continuation: cursor(list),
    })
}

fn entries_for(value: &Value, songs: bool) -> Vec<crate::core::discovery::Entry> {
    let mut entries = Vec::new();
    entries_in(value, &mut entries, songs);
    entries
}

pub(super) fn cursor(value: &Value) -> Option<String> {
    value
        .pointer("/continuations/0/nextContinuationData/continuation")
        .or_else(|| value.pointer("/continuations/0/nextRadioContinuationData/continuation"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(super) fn find<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(found) = value.get(key) {
        return Some(found);
    }
    match value {
        Value::Object(map) => map
            .iter()
            .filter(|(key, _)| can_visit(key))
            .find_map(|(_, child)| find(child, key)),
        Value::Array(values) => values.iter().find_map(|child| find(child, key)),
        _ => None,
    }
}

fn can_visit(key: &str) -> bool {
    !key.contains("menu") && !key.starts_with("on") && !key.contains("Endpoint")
}

pub(super) fn text(value: &Value) -> String {
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
        let page = parse(&json!({"contents":{"gridRenderer":{"items":[tile(json!({"watchEndpoint":{"videoId":"song","playlistId":"PLcollection"}}))]}}})).unwrap();
        assert_eq!(page.tracks.len(), 1);
        assert_eq!(page.tracks[0].artist_names(), "Artist");
        assert!(parse(&json!({"error":{}})).is_err());
    }

    #[test]
    fn radio_uses_the_requested_queue_not_unrelated_recommendations() {
        let song = |id, playlist| json!({"tileRenderer":{"onSelectCommand":{"watchEndpoint":{"videoId":id,"playlistId":playlist,"continuePlayback":true}},"metadata":{"tileMetadataRenderer":{"title":{"simpleText":"Song"}}}}});
        let value = json!({"contents":{"singleColumnWatchNextResults":{"pivot":{"sectionListRenderer":{"contents":[{"shelfRenderer":{"content":{"horizontalListRenderer":{"items":[song("unrelated","other")]}}}},{"shelfRenderer":{"content":{"horizontalListRenderer":{"items":[song("one","RDMIX"),song("two","RDMIX")],"continuations":[{"nextRadioContinuationData":{"continuation":"more"}}]}}}}]}}}}});
        let page = parse_radio(&value, "RDMIX").unwrap();
        assert_eq!(
            page.tracks
                .iter()
                .map(|track| track.id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two"]
        );
        assert_eq!(page.continuation.as_deref(), Some("more"));
        assert!(parse_radio(&value, "unknown").is_err());
    }
    #[test]
    fn radio_skips_matching_cards_without_tracks_to_find_the_queue() {
        let tile = |video: Option<&str>| {
            json!({"tileRenderer": {
                "onSelectCommand": {"watchEndpoint": {"videoId": video, "playlistId": "mix"}},
                "metadata": {"tileMetadataRenderer": {"title": {"simpleText": "Song"}}}
            }})
        };
        let value = json!([
            {"shelfRenderer": {"content": {"items": [tile(None)]}}},
            {"shelfRenderer": {"content": {"items": [tile(Some("playable"))]}}}
        ]);
        let page = parse_radio(&value, "mix").unwrap();
        assert_eq!(page.tracks[0].id.0, "playable");
    }
}
