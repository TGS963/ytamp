//! Domain types shared by the state, the API layer, and the player.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A YouTube video id, the identity of a playable song.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlaylistId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlbumId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtistId(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_id: Option<AlbumId>,
    pub duration: Option<Duration>,
    pub thumbnail_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: PlaylistId,
    pub title: String,
    pub track_count: Option<usize>,
    pub thumbnail_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Album {
    pub id: AlbumId,
    pub title: String,
    pub artists: Vec<String>,
    pub year: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Artist {
    pub id: ArtistId,
    pub name: String,
    pub thumbnail_url: Option<String>,
}

/// One page of search results across every result type.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResults {
    pub songs: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub playlists: Vec<Playlist>,
}

/// An artist's browse page: top songs, then albums and singles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArtistPage {
    pub id: ArtistId,
    pub name: String,
    pub thumbnail_url: Option<String>,
    pub top_songs: Vec<Track>,
    pub albums: Vec<Album>,
    pub singles: Vec<Album>,
}

/// An album's browse page: the album and its track list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlbumPage {
    pub album: Album,
    pub tracks: Vec<Track>,
}
