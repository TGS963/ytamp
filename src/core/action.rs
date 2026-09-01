//! Everything that can happen, as data.
//!
//! Views return actions. The effect runtime and the player send result
//! actions back. The reducer in `update.rs` consumes all of them.

use std::time::Duration;

use super::model::{PlaylistId, SearchResults, Track};
use super::state::Page;

#[derive(Clone, Debug)]
pub enum Action {
    // From the user.
    NavigatedTo(Page),
    CookieDraftChanged(String),
    CookiesSubmitted,
    SearchInputChanged(String),
    SearchSubmitted,
    PlaylistOpened(PlaylistId),
    ContextPlayed { tracks: Vec<Track>, start: usize },
    TrackQueued(Track),
    PlayToggled,
    NextPressed,
    PreviousPressed,
    SeekRequested(Duration),
    VolumeSet(f32),
    ShuffleToggled,
    RepeatCycled,
    NoticeDismissed(usize),

    // From the shell at startup.
    StoredCookiesFound(String),

    // From the effect runtime.
    NoticePosted(String),
    AuthVerified(Result<(), String>),
    SearchLoaded(Result<SearchResults, String>),
    PlaylistsLoaded(Result<Vec<crate::core::model::Playlist>, String>),
    LikedLoaded(Result<Vec<Track>, String>),
    PlaylistTracksLoaded(PlaylistId, Result<Vec<Track>, String>),

    // From the player engine.
    Player(PlayerEvent),
}

#[derive(Clone, Debug)]
pub enum PlayerEvent {
    /// The stream resolved and decoding started. Carries the real duration.
    TrackStarted {
        duration: Option<Duration>,
    },
    PositionChanged(Duration),
    TrackEnded,
    Failed(String),
}
