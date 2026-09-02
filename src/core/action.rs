//! Everything that can happen, as data.
//!
//! Views return actions. The effect runtime and the player send result
//! actions back. The reducer in `update.rs` consumes all of them.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::model::{
    AlbumId, AlbumPage, ArtistId, ArtistPage, ArtistRef, Playlist, PlaylistId, SearchResults,
    Track, TrackId,
};
use super::state::Page;
use crate::skin::Skin;

#[derive(Clone, Debug)]
pub enum Action {
    // From the user.
    NavigatedTo(Page),
    CookieDraftChanged(String),
    AuthUserDraftChanged(String),
    CookiesSubmitted,
    SignOutRequested,
    OAuthClientIdChanged(String),
    OAuthClientSecretChanged(String),
    OAuthStartRequested,
    /// The device flow produced its verification URL; the user opens
    /// it while the runtime polls for completion.
    OAuthUrlReady(String),
    SearchInputChanged(String),
    SearchSubmitted,
    PlaylistOpened(PlaylistId),
    ArtistOpened(ArtistId),
    /// A click on an artist name with no channel id: searches for
    /// the artist by name instead of opening a page directly.
    ArtistSearchRequested(String),
    /// An artist name in a track row or the player bar. With an id it
    /// opens the artist page, otherwise it searches for the name.
    ArtistLinkOpened(ArtistRef),
    AlbumOpened(AlbumId),
    /// Returns to the page Back left, popping `State.history`.
    BackPressed,
    ContextPlayed {
        tracks: Vec<Track>,
        start: usize,
    },
    TrackQueued(Track),
    /// The pointer rested on a track row long enough to warm its
    /// cache. The view emits this once per dwell.
    TrackHovered(Track),
    PlayToggled,
    NextPressed,
    PreviousPressed,
    /// A double click on a Winamp playlist row: plays the track at
    /// this index in the queue's upcoming list.
    QueueJumped(usize),
    /// The Winamp playlist's REM menu: drops every explicitly queued
    /// track, leaving the context alone.
    QueueCleared,
    SeekRequested(Duration),
    VolumeSet(f32),
    ShuffleToggled,
    RepeatCycled,
    AutoplayToggled,
    QueuePanelToggled,
    NoticeDismissed(usize),
    /// Opens or closes the Winamp skin window.
    WinampToggled,
    /// Sets the skin window's scale, clamped to 1 through 4.
    WinampScaleSet(u8),
    WinampOnTopToggled,
    /// Wears the built-in skin (`None`) or a named one from the skins
    /// folder.
    SkinChosen(Option<String>),
    /// A `.wsz` or `.zip` file dropped on a window, to install and
    /// wear.
    SkinFileDropped(PathBuf),
    /// A like or unlike from a row or the player bar. The reducer
    /// decides which, from the track's membership in the liked list.
    TrackLikeToggled(Track),
    /// A track added to a playlist from a row's context menu.
    TrackAddedToPlaylist {
        playlist: PlaylistId,
        track: Track,
    },
    /// A track removed from the open playlist, by its playlist item id.
    TrackRemovedFromPlaylist {
        playlist: PlaylistId,
        item_id: String,
    },
    /// The create-playlist dialog's Create button, or Enter in its
    /// text field. The title comes from the dialog's draft.
    PlaylistCreateRequested(String),
    /// Opens the create-playlist dialog. `Some(track)` remembers a
    /// track for the row that opened it, so the new playlist also
    /// receives it once it exists.
    CreatePlaylistDialogOpened(Option<Track>),
    CreatePlaylistDraftChanged(String),
    /// Cancels the dialog on screen, if any.
    DialogDismissed,

    // From the effect runtime, Winamp skins.
    /// The decoded skin lives only in `WinampShell`, never in
    /// `State`: `App::reduce` hands this to the shell before the
    /// reducer sees it. A failure becomes a plain `NoticePosted`
    /// instead.
    SkinLoaded(Result<Arc<Skin>, String>),
    /// A dropped skin finished copying into the skins folder. On
    /// success the reducer wears it; on failure it posts a notice.
    SkinInstalled(Result<String, String>),
    /// The skins folder's contents, freshly listed.
    SkinListRefreshed(Vec<String>),

    // From the shell at startup.
    StoredAuthFound(crate::core::effect::AuthMethod),
    SessionRestored(crate::core::session::SavedSession),

    // From the effect runtime.
    NoticePosted(String),
    AuthVerified(Result<(), String>),
    SearchLoaded(Result<SearchResults, String>),
    PlaylistsLoaded(Result<Vec<crate::core::model::Playlist>, String>),
    /// Custom cover art fetched for a batch of playlists. Always
    /// carries only the playlists a fetch actually found a cover for.
    PlaylistCoversLoaded(Vec<(PlaylistId, String)>),
    LikedLoaded(Result<Vec<Track>, String>),
    PlaylistTracksLoaded(PlaylistId, Result<Vec<Track>, String>),
    ArtistLoaded(ArtistId, Result<ArtistPage, String>),
    AlbumLoaded(AlbumId, Result<AlbumPage, String>),
    /// The radio fetched for the track that ended the queue, once
    /// autoplay ran out of upcoming tracks.
    RadioLoaded(TrackId, Result<Vec<Track>, String>),
    /// One page of the liked-songs list. `finished` marks the last
    /// page of the stream.
    LikedPageLoaded {
        tracks: Vec<Track>,
        finished: bool,
    },
    /// One page of an open playlist's tracks, the same way as
    /// `LikedPageLoaded`.
    PlaylistTracksPageLoaded {
        id: PlaylistId,
        tracks: Vec<Track>,
        finished: bool,
    },
    /// The library cache read, delivered once whether or not either
    /// file existed. A `None` field is a cache miss for that list.
    LibraryCacheLoaded {
        playlists: Option<Vec<Playlist>>,
        liked: Option<Vec<Track>>,
    },
    /// A playlist's cached track list, delivered only on a cache hit.
    PlaylistTracksCacheLoaded(PlaylistId, Vec<Track>),
    /// The server's answer to an `AddToPlaylist` request: the new
    /// row's item id, for the track and playlist the request named.
    PlaylistItemAdded {
        playlist: PlaylistId,
        track: Track,
        result: Result<String, String>,
    },
    /// The server's answer to a `CreatePlaylist` request.
    PlaylistCreated(Result<Playlist, String>),
    /// The server's answer to a fire-and-forget library write: a rate
    /// or a playlist-item removal. `what` names the list the write
    /// touched, so a failure knows what to refetch.
    LibraryWriteFinished {
        what: LibraryWrite,
        result: Result<(), String>,
    },

    // From the player engine.
    Player(PlayerEvent),
}

/// The library list an optimistic write changed. `LibraryWriteFinished`
/// uses this to refetch the truth after a failed write.
#[derive(Clone, Debug, PartialEq)]
pub enum LibraryWrite {
    Liked,
    Playlist(PlaylistId),
}

#[derive(Clone, Debug)]
pub enum PlayerEvent {
    /// The stream resolved and decoding started. Carries the real
    /// duration and the decoded stream's shape, for the Winamp
    /// window's mono/stereo lamps and kHz text.
    TrackStarted {
        duration: Option<Duration>,
        channels: u16,
        sample_rate: u32,
    },
    PositionChanged(Duration),
    TrackEnded,
    Failed(String),
}
