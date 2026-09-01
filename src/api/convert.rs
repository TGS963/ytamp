//! Pure conversions from ytmapi-rs response types to the app's models.

use std::time::Duration;

use ytmapi_rs::common::{Thumbnail, YoutubeID};
use ytmapi_rs::parse::{
    LibraryPlaylist, PlaylistItem, SearchResultAlbum, SearchResultArtist, SearchResultSong,
    SearchResults,
};

use crate::core::model::{
    Album, AlbumId, Artist, ArtistId, Playlist, PlaylistId, SearchResults as ModelSearchResults,
    Track, TrackId,
};

pub fn search_results(results: SearchResults) -> ModelSearchResults {
    ModelSearchResults {
        songs: results.songs.into_iter().map(song_to_track).collect(),
        albums: results.albums.into_iter().map(album_result).collect(),
        artists: results.artists.into_iter().map(artist_result).collect(),
        playlists: vec![],
    }
}

pub fn song_to_track(song: SearchResultSong) -> Track {
    Track {
        id: TrackId(song.video_id.get_raw().to_string()),
        title: song.title,
        artists: vec![song.artist],
        album: song.album.map(|album| album.name),
        duration: parse_duration(&song.duration),
        thumbnail_url: largest_thumbnail(&song.thumbnails),
    }
}

fn album_result(album: SearchResultAlbum) -> Album {
    Album {
        id: AlbumId(album.album_id.get_raw().to_string()),
        title: album.title,
        artists: vec![album.artist],
        year: Some(album.year),
        thumbnail_url: largest_thumbnail(&album.thumbnails),
    }
}

fn artist_result(artist: SearchResultArtist) -> Artist {
    Artist {
        id: ArtistId(artist.browse_id.get_raw().to_string()),
        name: artist.artist,
        thumbnail_url: largest_thumbnail(&artist.thumbnails),
    }
}

pub fn library_playlist(playlist: LibraryPlaylist) -> Playlist {
    Playlist {
        id: PlaylistId(playlist.playlist_id.get_raw().to_string()),
        title: playlist.title,
        track_count: leading_number(&playlist.tracks),
        thumbnail_url: largest_thumbnail(&playlist.thumbnails),
    }
}

/// Songs and videos become tracks. Episodes and uploads are out of the
/// v1 scope and disappear from the list.
pub fn playlist_item_to_track(item: PlaylistItem) -> Option<Track> {
    match item {
        PlaylistItem::Song(song) => Some(Track {
            id: TrackId(song.video_id.get_raw().to_string()),
            title: song.title,
            artists: song.artists.into_iter().map(|artist| artist.name).collect(),
            album: Some(song.album.name),
            duration: parse_duration(&song.duration),
            thumbnail_url: largest_thumbnail(&song.thumbnails),
        }),
        PlaylistItem::Video(video) => Some(Track {
            id: TrackId(video.video_id.get_raw().to_string()),
            title: video.title,
            artists: vec![video.channel_name],
            album: None,
            duration: parse_duration(&video.duration),
            thumbnail_url: largest_thumbnail(&video.thumbnails),
        }),
        PlaylistItem::Episode(_) | PlaylistItem::UploadSong(_) => None,
    }
}

/// Parses "3:45" or "1:02:03" into a duration.
pub fn parse_duration(text: &str) -> Option<Duration> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut seconds: u64 = 0;
    for part in parts {
        seconds = seconds * 60 + part.parse::<u64>().ok()?;
    }
    Some(Duration::from_secs(seconds))
}

fn largest_thumbnail(thumbnails: &[Thumbnail]) -> Option<String> {
    thumbnails
        .iter()
        .max_by_key(|thumbnail| thumbnail.width * thumbnail.height)
        .map(|thumbnail| thumbnail.url.clone())
}

/// Reads the number that starts a text like "42 tracks".
fn leading_number(text: &str) -> Option<usize> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse_from_colon_notation() {
        assert_eq!(parse_duration("3:45"), Some(Duration::from_secs(225)));
        assert_eq!(parse_duration("1:02:03"), Some(Duration::from_secs(3723)));
        assert_eq!(parse_duration("245"), Some(Duration::from_secs(245)));
        assert_eq!(parse_duration("bad"), None);
        assert_eq!(parse_duration("1:2:3:4"), None);
    }

    #[test]
    fn the_largest_thumbnail_wins() {
        let thumbnails = vec![
            Thumbnail {
                height: 60,
                width: 60,
                url: "small".into(),
            },
            Thumbnail {
                height: 544,
                width: 544,
                url: "large".into(),
            },
        ];
        assert_eq!(largest_thumbnail(&thumbnails), Some("large".into()));
        assert_eq!(largest_thumbnail(&[]), None);
    }

    #[test]
    fn track_counts_parse_from_byline_text() {
        assert_eq!(leading_number("42 tracks"), Some(42));
        assert_eq!(leading_number("no digits"), None);
    }
}
