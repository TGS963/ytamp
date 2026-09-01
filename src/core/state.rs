//! The single application state.
//!
//! Views read this state and return actions. Only the reducer in
//! `update.rs` writes it.

use std::time::Duration;

use super::model::{Playlist, PlaylistId, SearchResults, Track};
use super::queue::Queue;

/// A value that arrives over the network.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Loadable<T> {
    #[default]
    NotAsked,
    Loading,
    Loaded(T),
    Failed(String),
}

impl<T> Loadable<T> {
    pub fn loaded(&self) -> Option<&T> {
        match self {
            Self::Loaded(value) => Some(value),
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
    /// The tracks of the playlist page the user has open.
    pub open_playlist: Loadable<Vec<Track>>,
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
    pub playback: PlaybackState,
    pub queue_open: bool,
    /// User-visible problem reports, newest last.
    pub notices: Vec<String>,
}
