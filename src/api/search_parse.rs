//! A tolerant, row-by-row reader of the raw filtered-search JSON.
//!
//! ytmapi-rs parses a search shelf as one strict struct, so one odd row
//! (a video mixed into the songs shelf, an audiobook mixed into the
//! albums shelf) fails the whole shelf. This module reads the same
//! JSON by hand, row by row, and skips a row that does not fit
//! instead of failing the shelf.

use serde_json::Value;
use ytmapi_rs::common::Thumbnail;

use super::convert;
use crate::core::model::{
    Album, AlbumId, Artist, ArtistId, ArtistRef, Playlist, PlaylistId, Track, TrackId,
};
use crate::thumbnails;

/// Where a filtered search's shelf lives in the raw response, from the
/// anonymous "songs", "albums", or "artists" filtered search.
const SECTION_LIST_POINTER: &str =
    "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";

/// Every song row that carries a title and a video id. A row missing
/// either one is skipped, not fatal to the rest of the shelf.
pub fn parse_songs(value: &Value) -> Vec<Track> {
    parse_shelf(value, "song", song_from_row)
}

/// Every album row whose browse id names an album. A row of another
/// kind (an audiobook, a podcast) is skipped, not fatal to the shelf.
pub fn parse_albums(value: &Value) -> Vec<Album> {
    parse_shelf(value, "album", album_from_row)
}

/// Every artist row whose browse id names an artist channel.
pub fn parse_artists(value: &Value) -> Vec<Artist> {
    parse_shelf(value, "artist", artist_from_row)
}

/// Playlist browse endpoints carry a VL prefix; playback APIs expect the ID.
pub fn parse_playlists(value: &Value) -> Vec<Playlist> {
    parse_shelf(value, "playlist", |row| {
        let id = row_browse_id(row)?;
        let id = id.strip_prefix("VL")?;
        if id.is_empty() {
            return None;
        }
        Some(Playlist {
            id: PlaylistId(id.to_string()),
            title: row_title(row)?,
            track_count: None,
            thumbnail_url: thumbnails::preferred(&row_thumbnails(row)),
        })
    })
}

/// True when the search shelf in `value` carries at least one row.
/// Tells apart a real empty result from a shelf every row of which
/// failed to parse, so a caller falls back only in the second case.
pub fn shelf_has_rows(value: &Value) -> bool {
    !shelf_rows(value).is_empty()
}

/// Runs `parse_row` over every row of the search shelf in `value`,
/// keeping the rows that parse and logging the index of one that does
/// not, under the given `kind` name.
fn parse_shelf<T>(value: &Value, kind: &str, parse_row: impl Fn(&Value) -> Option<T>) -> Vec<T> {
    shelf_rows(value)
        .into_iter()
        .enumerate()
        .filter_map(|(index, row)| log_skipped_row(kind, index, parse_row(row)))
        .collect()
}

/// `parsed`, unchanged. Logs the row index first when `parsed` is
/// `None`, so a skipped row leaves a trace without failing the shelf.
fn log_skipped_row<T>(kind: &str, index: usize, parsed: Option<T>) -> Option<T> {
    if parsed.is_none() {
        log::debug!("search {kind} row {index} did not parse, skipping it");
    }
    parsed
}

/// One song row: title, video id, artists and album from the second
/// flex column, and thumbnails. A row missing a title or a video id
/// is not a song row, so it comes back as `None`.
fn song_from_row(row: &Value) -> Option<Track> {
    let title = row_title(row)?;
    let video_id = row_video_id(row)?;
    let runs = column_runs(row, 1);
    let (artists, album, duration) = parse_song_runs(runs);
    let thumbnails = row_thumbnails(row);
    Some(Track {
        source: Default::default(),
        id: TrackId(video_id.clone()),
        title,
        artists,
        album: album.as_ref().map(|album| album.name.clone()),
        album_id: album.map(|album| album.id),
        duration,
        thumbnail_url: thumbnails::track_art(&thumbnails, &video_id),
        playlist_item_id: None,
    })
}

/// One album row: an album's own browse id names it, so a row whose
/// id does not start with "MPRE" is some other result type and comes
/// back as `None`.
fn album_from_row(row: &Value) -> Option<Album> {
    let browse_id = row_browse_id(row)?;
    if !browse_id.starts_with("MPRE") {
        return None;
    }
    let title = row_title(row)?;
    let (artists, year) = parse_album_runs(column_runs(row, 1));
    let thumbnails = row_thumbnails(row);
    Some(Album {
        id: AlbumId(browse_id.to_string()),
        title,
        artists,
        year,
        thumbnail_url: thumbnails::preferred(&thumbnails),
    })
}

/// One artist row: an artist's own browse id is a channel id, so a
/// row whose id does not start with "UC" comes back as `None`.
fn artist_from_row(row: &Value) -> Option<Artist> {
    let browse_id = row_browse_id(row)?;
    if !browse_id.starts_with("UC") {
        return None;
    }
    let name = row_title(row)?;
    let thumbnails = row_thumbnails(row);
    Some(Artist {
        id: ArtistId(browse_id.to_string()),
        name,
        thumbnail_url: thumbnails::preferred(&thumbnails),
    })
}

/// One song row's second flex column, split into its artists, its
/// album, and its duration. A run with no navigation endpoint and no
/// recognizable shape (a view count, a play count) names nothing and
/// is skipped.
fn parse_song_runs(
    runs: &[Value],
) -> (Vec<ArtistRef>, Option<NamedId>, Option<std::time::Duration>) {
    let mut artists = Vec::new();
    let mut album = None;
    let mut duration = None;
    for run in content_runs(runs) {
        classify_song_run(run, &mut artists, &mut album, &mut duration);
    }
    (artists, album, duration)
}

/// A browse id paired with its display name, for a song's album run.
struct NamedId {
    id: AlbumId,
    name: String,
}

/// Adds one song run's meaning to the accumulators: an artist or an
/// album when the run carries a browse id, else a duration when the
/// text has that shape, else nothing.
fn classify_song_run(
    run: &Value,
    artists: &mut Vec<ArtistRef>,
    album: &mut Option<NamedId>,
    duration: &mut Option<std::time::Duration>,
) {
    let Some(text) = run_text(run) else {
        return;
    };
    if let Some(browse_id) = run_browse_id(run) {
        if browse_id.starts_with("MPRE") {
            *album = Some(NamedId {
                id: AlbumId(browse_id.to_string()),
                name: text.to_string(),
            });
        } else {
            artists.push(ArtistRef {
                name: text.to_string(),
                id: Some(ArtistId(browse_id.to_string())),
            });
        }
        return;
    }
    if let Some(parsed) = duration_text(text) {
        *duration = Some(parsed);
        return;
    }
    if is_count_text(text) {
        return;
    }
    artists.push(ArtistRef::named(text));
}

/// `text` as a duration, when it has the "m:ss" or "h:mm:ss" shape. A
/// bare number with no colon (a year, a count) is not a duration.
fn duration_text(text: &str) -> Option<std::time::Duration> {
    if !text.contains(':') {
        return None;
    }
    convert::parse_duration(text)
}

/// True for a view or play count run ("6.9M views", "2.8B plays",
/// "317M monthly audience"), so it is not mistaken for an artist name.
fn is_count_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.ends_with("views") || lower.ends_with("plays") || lower.ends_with("audience")
}

/// One album row's second flex column, split into its artist names
/// and its release year. Only a linked run names an artist; the
/// unlinked album-type word at the front ("Album", "Single", or an
/// unrecognized type such as "Audiobook") is skipped rather than
/// guessed at.
fn parse_album_runs(runs: &[Value]) -> (Vec<String>, Option<String>) {
    let mut artists = Vec::new();
    let mut year = None;
    for run in content_runs(runs) {
        let Some(text) = run_text(run) else {
            continue;
        };
        if run_browse_id(run).is_some() {
            artists.push(text.to_string());
        } else if is_four_digit_year(text) {
            year = Some(text.to_string());
        }
    }
    (artists, year)
}

fn is_four_digit_year(text: &str) -> bool {
    text.len() == 4 && text.chars().all(|c| c.is_ascii_digit())
}

/// The content runs of a flex column's run list: every other run,
/// starting at zero, since YouTube Music separates them with a plain
/// " • " run at each odd index.
fn content_runs(runs: &[Value]) -> impl Iterator<Item = &Value> {
    runs.iter().step_by(2)
}

/// The rows of the first `musicShelfRenderer` in a filtered search
/// response, or none when the response carries no such shelf.
fn shelf_rows(value: &Value) -> Vec<&Value> {
    let Some(sections) = value
        .pointer(SECTION_LIST_POINTER)
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let Some(shelf_contents) = sections.iter().find_map(music_shelf_contents) else {
        return Vec::new();
    };
    shelf_contents
        .iter()
        .filter_map(|item| item.get("musicResponsiveListItemRenderer"))
        .collect()
}

fn music_shelf_contents(section: &Value) -> Option<&Vec<Value>> {
    section.pointer("/musicShelfRenderer/contents")?.as_array()
}

/// A row's title: the first run of its first flex column.
fn row_title(row: &Value) -> Option<String> {
    let text = run_text(column_runs(row, 0).first()?)?;
    Some(text.to_string())
}

/// A row's own browse id, for a row whose whole entry opens a browse
/// page (an album, an artist).
fn row_browse_id(row: &Value) -> Option<&str> {
    row.pointer("/navigationEndpoint/browseEndpoint/browseId")?
        .as_str()
}

/// A song row's video id: from the row's own play data when present,
/// else from the play button under its thumbnail overlay.
fn row_video_id(row: &Value) -> Option<String> {
    let from_play_data = row
        .pointer("/playlistItemData/videoId")
        .and_then(Value::as_str);
    let from_overlay = || {
        row.pointer(
            "/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer\
             /playNavigationEndpoint/watchEndpoint/videoId",
        )
        .and_then(Value::as_str)
    };
    from_play_data.or_else(from_overlay).map(str::to_string)
}

/// A row's thumbnail list, or empty when the row carries none.
fn row_thumbnails(row: &Value) -> Vec<Thumbnail> {
    row.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default()
}

/// One flex column's run list, or an empty slice when the column is
/// absent or carries no runs.
fn column_runs(row: &Value, column: usize) -> &[Value] {
    row.get("flexColumns")
        .and_then(|columns| columns.get(column))
        .and_then(|column| column.get("musicResponsiveListItemFlexColumnRenderer"))
        .and_then(|renderer| renderer.get("text"))
        .and_then(|text| text.get("runs"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn run_text(run: &Value) -> Option<&str> {
    run.get("text")?.as_str()
}

fn run_browse_id(run: &Value) -> Option<&str> {
    run.pointer("/navigationEndpoint/browseEndpoint/browseId")?
        .as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {path}: {error}"));
        serde_json::from_str(&text).expect("valid json")
    }

    #[test]
    fn playlist_search_normalizes_ids_and_skips_non_playlist_rows() {
        let row = |id: &str| {
            serde_json::json!({"musicResponsiveListItemRenderer": {
                "navigationEndpoint": {"browseEndpoint": {"browseId": id}},
                "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Mix"}]}}}]
            }})
        };
        let json = serde_json::json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [{"musicShelfRenderer": {"contents": [row("VLPLtest"), row("UCother"), row("VL")]}}]}}}}]}}});
        let playlists = parse_playlists(&json);
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].id.0, "PLtest");
        assert_eq!(playlists[0].title, "Mix");
    }

    #[test]
    fn every_coldplay_song_row_carries_a_video_id() {
        let songs = parse_songs(&fixture("search_songs_coldplay.json"));
        assert_eq!(songs.len(), 20);
    }

    #[test]
    fn a_known_coldplay_song_row_parses_in_full() {
        let songs = parse_songs(&fixture("search_songs_coldplay.json"));
        let hymn = songs
            .iter()
            .find(|track| track.title == "Hymn for the Weekend")
            .expect("the row is present");
        assert_eq!(hymn.id, TrackId("H3Kzh6RrnMc".into()));
        assert_eq!(
            hymn.artists,
            vec![ArtistRef {
                name: "Coldplay".into(),
                id: Some(ArtistId("UCIaFw5VBEK8qaW6nRpx_qnw".into())),
            }]
        );
        assert_eq!(hymn.album, Some("A Head Full of Dreams".into()));
        assert_eq!(hymn.album_id, Some(AlbumId("MPREb_PvMNqFUp1oW".into())));
        assert_eq!(hymn.duration, Some(Duration::from_secs(4 * 60 + 19)));
    }

    #[test]
    fn coldplay_albums_and_artists_parse() {
        let albums = parse_albums(&fixture("search_albums_coldplay.json"));
        assert_eq!(albums.len(), 20);
        let artists = parse_artists(&fixture("search_artists_coldplay.json"));
        assert_eq!(artists.len(), 7);
    }

    /// The query that reproduces the real-world failure: a plain
    /// video ("If the \"Harry Potter\" Song Had Lyrics", a music
    /// video with no album) sits in the songs shelf next to real
    /// songs. ytmapi-rs fails the whole shelf on it; this parser
    /// keeps every row, including that one, with no album.
    #[test]
    fn the_video_row_in_a_songs_shelf_still_parses() {
        let songs = parse_songs(&fixture("search_songs_harry_potter_audiobook.json"));
        assert_eq!(songs.len(), 23);
        let video = songs
            .iter()
            .find(|track| track.title == "If the \"Harry Potter\" Song Had Lyrics")
            .expect("the video row is present");
        assert_eq!(video.id, TrackId("xD0QXHPz7y8".into()));
        assert_eq!(video.album, None);
        assert_eq!(video.album_id, None);
        assert_eq!(video.duration, Some(Duration::from_secs(2 * 60 + 16)));
        assert_eq!(
            video.artists,
            vec![ArtistRef {
                name: "The Warp Zone".into(),
                id: Some(ArtistId("UCNbfqEVspNoKZG1si9xggow".into())),
            }]
        );
    }

    #[test]
    fn harry_potter_audiobook_albums_parse_with_no_typed_album_type() {
        let albums = parse_albums(&fixture("search_albums_harry_potter_audiobook.json"));
        assert_eq!(albums.len(), 19);
    }

    /// A synthetic fixture with a row that carries no title and no
    /// video id (an empty flex column). The other two rows still
    /// parse, so one bad row never costs the whole shelf.
    #[test]
    fn a_row_with_no_title_is_skipped_without_losing_the_others() {
        let songs = parse_songs(&fixture("search_songs_broken_row.json"));
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].title, "Good Song One");
        assert_eq!(songs[1].title, "Good Song Two");
        assert_eq!(songs[1].album, None);
    }
}
