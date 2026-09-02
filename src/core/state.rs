//! The single application state.
//!
//! Views read this state and return actions. Only the reducer in
//! `update.rs` writes it.

use std::time::Duration;

use super::model::{
    AlbumId, AlbumPage, ArtistId, ArtistPage, Playlist, PlaylistId, SearchResults, Track, TrackId,
};
use super::queue::Queue;

/// A value that arrives over the network.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Loadable<T> {
    #[default]
    NotAsked,
    Loading,
    /// A cached value shows while the network refresh still runs. The
    /// UI renders this like `Loaded`, with no spinner.
    Refreshing(T),
    Loaded(T),
    Failed(String),
}

impl<T> Loadable<T> {
    pub fn loaded(&self) -> Option<&T> {
        match self {
            Self::Loaded(value) | Self::Refreshing(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum AuthState {
    #[default]
    SignedOut,
    Verifying,
    SignedIn,
    Failed(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Page {
    #[default]
    SignIn,
    Search,
    Library,
    Playlist(PlaylistId),
    Artist(ArtistId),
    Album(AlbumId),
}

/// What the user is entering on the sign-in page.
#[derive(Clone, Debug, Default)]
pub struct SignInState {
    /// The Cookie header value (the fallback path).
    pub draft: String,
    /// The X-Goog-AuthUser header value: which signed-in Google
    /// account the session belongs to. Empty means account 0.
    pub authuser_draft: String,
    /// The user's own Google Cloud OAuth client (TV type).
    pub client_id_draft: String,
    pub client_secret_draft: String,
    /// The verification URL of a running device flow, once known.
    pub oauth_url: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchState {
    pub input: String,
    pub results: Loadable<SearchResults>,
}

#[derive(Clone, Debug, Default)]
pub struct LibraryState {
    pub playlists: Loadable<Vec<Playlist>>,
    pub liked: Loadable<Vec<Track>>,
    /// True while more pages of the liked list are still arriving,
    /// so the UI can show a spinner row under the visible tracks.
    pub liked_loading_more: bool,
    /// Pages that have arrived while `liked` still shows cached data,
    /// held here until the stream finishes and they swap in together.
    pub incoming_liked: Vec<Track>,
    /// The tracks of the playlist page the user has open.
    pub open_playlist: Loadable<Vec<Track>>,
    /// True while more pages of the open playlist are still arriving.
    pub open_playlist_loading_more: bool,
    /// Pages that have arrived while `open_playlist` still shows
    /// cached data, held here until the stream finishes.
    pub incoming_playlist: Vec<Track>,
}

/// The artist and album pages the user has opened, each loaded on its
/// own so a visit to one never disturbs the other's cached page.
#[derive(Clone, Debug, Default)]
pub struct BrowseState {
    pub artist: Loadable<ArtistPage>,
    pub album: Loadable<AlbumPage>,
    /// The name behind the artist link the user clicked. A channel id
    /// from the library is not always an artist, so a failed page
    /// falls back to a search for this name.
    pub artist_fallback: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayStatus {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Clone, Debug)]
pub struct PlaybackState {
    pub queue: Queue,
    pub status: PlayStatus,
    pub position: Duration,
    pub track_duration: Option<Duration>,
    pub volume: f32,
    /// Where the next started track seeks to: set when a restored
    /// session resumes mid-track.
    pub resume_position: Option<Duration>,
    /// The track id of the last hover prefetch. Guards against a
    /// second prefetch request for a row the pointer left and then
    /// entered again.
    pub last_hover_prefetch: Option<TrackId>,
    /// When true, the player fetches a radio of related songs after
    /// the queue ends, instead of stopping.
    pub autoplay: bool,
    /// The id of the track a running radio fetch started from. Guards
    /// a late result against a queue the user has since replaced.
    pub radio_request: Option<TrackId>,
    /// The decoded stream's channel count, zero until a track starts.
    pub channels: u16,
    /// The decoded stream's sample rate in Hz, zero until a track starts.
    pub sample_rate: u32,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            queue: Queue::default(),
            status: PlayStatus::default(),
            position: Duration::ZERO,
            track_duration: None,
            volume: 1.0,
            resume_position: None,
            last_hover_prefetch: None,
            autoplay: true,
            radio_request: None,
            channels: 0,
            sample_rate: 0,
        }
    }
}

/// The Winamp skin window's settings: whether it is open, and how it
/// is shown. Saved in the session.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WinampSettings {
    pub open: bool,
    /// Screen pixels per skin pixel, from 1 to 4.
    pub scale: u8,
    pub on_top: bool,
    /// The skin file name in the skins folder, or `None` for the
    /// built-in skin.
    pub skin: Option<String>,
}

impl Default for WinampSettings {
    fn default() -> Self {
        Self {
            open: false,
            scale: 2,
            on_top: false,
            skin: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub auth: AuthState,
    pub sign_in: SignInState,
    pub page: Page,
    pub search: SearchState,
    pub library: LibraryState,
    pub browse: BrowseState,
    pub playback: PlaybackState,
    pub queue_open: bool,
    pub winamp: WinampSettings,
    /// Pages the user navigated away from, most recent last. Back
    /// pops the top entry. Sidebar navigation clears it, so Back never
    /// crosses a deliberate jump to a different section.
    pub history: Vec<Page>,
    /// User-visible problem reports, newest last.
    pub notices: Vec<String>,
}
