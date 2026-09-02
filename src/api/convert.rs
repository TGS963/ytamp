//! Pure conversions from ytmapi-rs response types to the app's models.

use std::time::Duration;

use ytmapi_rs::common::{Thumbnail, YoutubeID};
use ytmapi_rs::parse::{
    AlbumResult, AlbumSong, ArtistSong, GetAlbum, GetArtist, LibraryPlaylist, ParsedSongAlbum,
    PlaylistItem, SearchResultAlbum, SearchResultArtist, SearchResultSong, SearchResultVideo,
    WatchPlaylistTrack,
};

use crate::core::model::{
    Album, AlbumId, AlbumPage, Artist, ArtistId, ArtistPage, Playlist, PlaylistId,
    SearchResults as ModelSearchResults, Track, TrackId,
};

pub fn search_results_from_tracks(
    songs: Vec<Track>,
    albums: Vec<SearchResultAlbum>,
    artists: Vec<SearchResultArtist>,
) -> ModelSearchResults {
    ModelSearchResults {
        songs,
        albums: albums.into_iter().map(album_result).collect(),
        artists: artists.into_iter().map(artist_result).collect(),
        playlists: vec![],
    }
}

pub fn song_to_track(song: SearchResultSong) -> Track {
    let (album, album_id) = split_song_album(song.album);
    Track {
        id: TrackId(song.video_id.get_raw().to_string()),
        title: song.title,
        artists: vec![song.artist],
        album,
        album_id,
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

/// A video search result as a track: the fallback when the song
/// search parse breaks. Episodes disappear from the list.
pub fn video_to_track(video: SearchResultVideo) -> Option<Track> {
    match video {
        SearchResultVideo::Video {
            title,
            channel_name,
            video_id,
            length,
            thumbnails,
            ..
        } => Some(Track {
            id: TrackId(video_id.get_raw().to_string()),
            title,
            artists: vec![channel_name],
            album: None,
            album_id: None,
            duration: parse_duration(&length),
            thumbnail_url: largest_thumbnail(&thumbnails),
        }),
        SearchResultVideo::VideoEpisode { .. } => None,
    }
}

/// Songs and videos become tracks. Episodes and uploads are out of the
/// v1 scope and disappear from the list.
pub fn playlist_item_to_track(item: PlaylistItem) -> Option<Track> {
    match item {
        PlaylistItem::Song(song) => {
            let (album, album_id) = split_song_album(Some(song.album));
            Some(Track {
                id: TrackId(song.video_id.get_raw().to_string()),
                title: song.title,
                artists: song.artists.into_iter().map(|artist| artist.name).collect(),
                album,
                album_id,
                duration: parse_duration(&song.duration),
                thumbnail_url: largest_thumbnail(&song.thumbnails),
            })
        }
        PlaylistItem::Video(video) => Some(Track {
            id: TrackId(video.video_id.get_raw().to_string()),
            title: video.title,
            artists: vec![video.channel_name],
            album: None,
            album_id: None,
            duration: parse_duration(&video.duration),
            thumbnail_url: largest_thumbnail(&video.thumbnails),
        }),
        PlaylistItem::Episode(_) | PlaylistItem::UploadSong(_) => None,
    }
}

/// Splits a parsed song album into its display name and id, so a
/// track can show its album title and later open that album page.
fn split_song_album(album: Option<ParsedSongAlbum>) -> (Option<String>, Option<AlbumId>) {
    match album {
        Some(album) => (
            Some(album.name),
            Some(AlbumId(album.id.get_raw().to_string())),
        ),
        None => (None, None),
    }
}

/// An artist's browse page: name, art, top songs, then albums and
/// singles. Top songs carry no duration and no thumbnail in the
/// artist response, so those fields stay empty.
pub fn artist_page(artist: GetArtist, id: ArtistId) -> ArtistPage {
    let releases = artist.top_releases;
    ArtistPage {
        id,
        name: artist.name,
        thumbnail_url: largest_thumbnail(&artist.thumbnails),
        top_songs: releases
            .songs
            .map(|songs| songs.results.into_iter().map(artist_song_to_track).collect())
            .unwrap_or_default(),
        albums: releases
            .albums
            .map(|albums| albums.results.into_iter().map(artist_album_result).collect())
            .unwrap_or_default(),
        singles: releases
            .singles
            .map(|singles| singles.results.into_iter().map(artist_album_result).collect())
            .unwrap_or_default(),
    }
}

fn artist_song_to_track(song: ArtistSong) -> Track {
    Track {
        id: TrackId(song.video_id.get_raw().to_string()),
        title: song.title,
        artists: song.artists.into_iter().map(|artist| artist.name).collect(),
        album: Some(song.album.name),
        album_id: Some(AlbumId(song.album.id.get_raw().to_string())),
        duration: None,
        thumbnail_url: None,
    }
}

fn artist_album_result(album: AlbumResult) -> Album {
    Album {
        id: AlbumId(album.album_id.get_raw().to_string()),
        title: album.title,
        artists: vec![],
        year: Some(album.year),
        thumbnail_url: largest_thumbnail(&album.thumbnails),
    }
}

/// An album's browse page. Album tracks carry no thumbnail in the
/// album response, so each one gets the album's own art and id.
pub fn album_page(album: GetAlbum, id: AlbumId) -> AlbumPage {
    let page_album = Album {
        id: id.clone(),
        title: album.title,
        artists: album.artists.into_iter().map(|artist| artist.name).collect(),
        year: Some(album.year),
        thumbnail_url: largest_thumbnail(&album.thumbnails),
    };
    let tracks = tracks_with_album_art(
        album.tracks.into_iter().map(album_song_to_track).collect(),
        &page_album,
    );
    AlbumPage {
        album: page_album,
        tracks,
    }
}

fn album_song_to_track(song: AlbumSong) -> Track {
    Track {
        id: TrackId(song.video_id.get_raw().to_string()),
        title: song.title,
        artists: vec![],
        album: None,
        album_id: None,
        duration: parse_duration(&song.duration),
        thumbnail_url: None,
    }
}

/// Stamps every track with its album's name, id, and art. Pulled out
/// as its own pure step because `GetAlbum` and `AlbumSong` are
/// `#[non_exhaustive]` in ytmapi-rs, so a unit test builds `Track` and
/// `Album` values directly instead of the ytmapi-rs response types.
fn tracks_with_album_art(tracks: Vec<Track>, album: &Album) -> Vec<Track> {
    tracks
        .into_iter()
        .map(|track| Track {
            album: Some(album.title.clone()),
            album_id: Some(album.id.clone()),
            thumbnail_url: album.thumbnail_url.clone(),
            ..track
        })
        .collect()
}

/// A "Start radio" track. It carries no album information at all.
pub fn watch_track(track: WatchPlaylistTrack) -> Track {
    Track {
        id: TrackId(track.video_id.get_raw().to_string()),
        title: track.title,
        artists: vec![track.author],
        album: None,
        album_id: None,
        duration: parse_duration(&track.duration),
        thumbnail_url: largest_thumbnail(&track.thumbnails),
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

    // `GetAlbum` and `AlbumSong` are `#[non_exhaustive]` in ytmapi-rs, so
    // this test builds `Track` and `Album` directly and exercises the
    // pure helper `tracks_with_album_art` in place of `album_page`.
    #[test]
    fn album_art_and_id_land_on_every_track() {
        let album = Album {
            id: AlbumId("album-1".into()),
            title: "Origins".into(),
            artists: vec!["Artist".into()],
            year: Some("2020".into()),
            thumbnail_url: Some("art-url".into()),
        };
        let bare_track = Track {
            id: TrackId("t1".into()),
            title: "Song".into(),
            artists: vec![],
            album: None,
            album_id: None,
            duration: None,
            thumbnail_url: None,
        };
        let stamped = tracks_with_album_art(vec![bare_track], &album);
        assert_eq!(stamped[0].album, Some("Origins".into()));
        assert_eq!(stamped[0].album_id, Some(AlbumId("album-1".into())));
        assert_eq!(stamped[0].thumbnail_url, Some("art-url".into()));
    }

    #[test]
    fn a_song_with_no_album_carries_no_album_id() {
        assert_eq!(split_song_album(None), (None, None));
    }

    #[test]
    fn a_song_album_splits_into_a_name_and_an_id() {
        let album = ParsedSongAlbum {
            name: "Origins".into(),
            id: ytmapi_rs::common::AlbumID::from_raw("album-1"),
        };
        assert_eq!(
            split_song_album(Some(album)),
            (Some("Origins".into()), Some(AlbumId("album-1".into())))
        );
    }
}
