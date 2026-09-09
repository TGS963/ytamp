//! Domain types shared by the state, the API layer, and the player.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

/// Media identity: a YouTube video id or a namespaced local path.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackId(pub String);
impl TrackId {
    pub fn local_file(path: &Path) -> Self {
        let encoded: String = path
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self(format!("local:{encoded}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlaylistId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlbumId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtistId(pub String);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum MediaSource {
    #[default]
    YouTube,
    LocalFile {
        path: PathBuf,
    },
}

/// One artist credit on a track: a display name, and the artist's
/// channel id when the source carries one. A click on a name with an
/// id opens that artist page; a click on a bare name searches for it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "ArtistRefRepr")]
pub struct ArtistRef {
    pub name: String,
    pub id: Option<ArtistId>,
}

impl ArtistRef {
    /// An artist credit with a name only, for a source that carries
    /// no channel id.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
        }
    }
}

/// The two shapes an `ArtistRef` reads from JSON: a bare name string,
/// from a session or cache saved before `ArtistRef` existed, or the
/// current struct shape. `ArtistRef` always writes the struct shape.
#[derive(Deserialize)]
#[serde(untagged)]
enum ArtistRefRepr {
    Name(String),
    Full { name: String, id: Option<ArtistId> },
}

impl From<ArtistRefRepr> for ArtistRef {
    fn from(repr: ArtistRefRepr) -> Self {
        match repr {
            ArtistRefRepr::Name(name) => ArtistRef::named(name),
            ArtistRefRepr::Full { name, id } => ArtistRef { name, id },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub source: MediaSource,
    pub id: TrackId,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub album: Option<String>,
    /// Absent in sessions saved before the field existed.
    #[serde(default)]
    pub album_id: Option<AlbumId>,
    pub duration: Option<Duration>,
    pub thumbnail_url: Option<String>,
    /// The id of this track's row in the playlist it came from. Only
    /// a removal needs it.
    #[serde(default)]
    pub playlist_item_id: Option<String>,
}

impl Track {
    pub fn is_local(&self) -> bool {
        matches!(self.source, MediaSource::LocalFile { .. })
    }

    pub fn local_path(&self) -> Option<&Path> {
        match &self.source {
            MediaSource::LocalFile { path } => Some(path),
            MediaSource::YouTube => None,
        }
    }

    pub fn playback_key(&self) -> String {
        self.local_path()
            .map_or_else(|| self.id.0.clone(), |path| TrackId::local_file(path).0)
    }

    /// The track's artist names, joined for a caption-line display.
    pub fn artist_names(&self) -> String {
        self.artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_artist_ref_deserializes_from_a_bare_name() {
        let artist: ArtistRef = serde_json::from_str("\"Radiohead\"").expect("parse");
        assert_eq!(artist, ArtistRef::named("Radiohead"));
    }

    #[test]
    fn an_artist_ref_deserializes_from_the_struct_shape() {
        let json = r#"{"name":"Radiohead","id":"UC-artist"}"#;
        let artist: ArtistRef = serde_json::from_str(json).expect("parse");
        assert_eq!(
            artist,
            ArtistRef {
                name: "Radiohead".into(),
                id: Some(ArtistId("UC-artist".into())),
            }
        );
    }

    #[test]
    fn an_artist_ref_round_trips_through_json() {
        let artist = ArtistRef {
            name: "Radiohead".into(),
            id: Some(ArtistId("UC-artist".into())),
        };
        let json = serde_json::to_string(&artist).expect("serialize");
        let restored: ArtistRef = serde_json::from_str(&json).expect("parse");
        assert_eq!(restored, artist);
    }

    #[test]
    fn a_bare_name_artist_ref_round_trips_as_the_struct_shape() {
        let artist = ArtistRef::named("Radiohead");
        let json = serde_json::to_string(&artist).expect("serialize");
        assert_eq!(json, r#"{"name":"Radiohead","id":null}"#);
    }

    #[test]
    fn track_artist_names_join_with_a_comma() {
        let track = Track {
            source: Default::default(),
            id: TrackId("t1".into()),
            title: "Song".into(),
            artists: vec![ArtistRef::named("A"), ArtistRef::named("B")],
            album: None,
            album_id: None,
            duration: None,
            thumbnail_url: None,
            playlist_item_id: None,
        };
        assert_eq!(track.artist_names(), "A, B");
    }

    #[test]
    fn old_track_json_defaults_to_youtube() {
        let json = r#"{"id":"t1","title":"Song","artists":[],"album":null,"album_id":null,"duration":null,"thumbnail_url":null,"playlist_item_id":null}"#;
        let track: Track = serde_json::from_str(json).unwrap();
        assert_eq!(track.source, MediaSource::YouTube);
        assert_eq!(track.playback_key(), "t1");
    }

    #[test]
    fn local_track_uses_a_path_key() {
        let track = Track {
            source: MediaSource::LocalFile {
                path: PathBuf::from("/tmp/song.wav"),
            },
            id: TrackId("song".into()),
            title: "Song".into(),
            artists: vec![],
            album: None,
            album_id: None,
            duration: None,
            thumbnail_url: None,
            playlist_item_id: None,
        };
        assert!(track.is_local());
        assert_eq!(track.local_path().unwrap(), Path::new("/tmp/song.wav"));
        assert_eq!(track.playback_key(), "local:2f746d702f736f6e672e776176");
    }
}
