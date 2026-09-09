//! The reducer: the only writer of the application state.
//!
//! `update` applies one action and returns the effects it causes.
//! It touches no clock, no network, and no audio device. Randomness
//! comes in as a parameter, so every path is testable.

use std::time::Duration;

use super::action::{Action, LibraryWrite, PlayerEvent};
use super::effect::{ApiRequest, Effect, LibraryCacheWrite, PlayerCommand};
use super::model::{
    AlbumId, AlbumPage, ArtistId, ArtistPage, ArtistRef, Playlist, PlaylistId, Track, TrackId,
};
use super::queue::{Queue, RandomBelow};
use super::state::{AuthState, Dialog, Loadable, Page, PlayStatus, State};

mod audio_controls;
mod auth;
mod library;
mod library_write;
mod navigation;
mod playback;
mod queue_controls;
mod radio;
mod skins;
#[cfg(test)]
mod tests;
use library::fetch_missing_library;
#[cfg(test)]
use library::ids_without_cover;
#[cfg(test)]
use library::merge_covers;
use library::start_playlist_load;
pub use library_write::is_liked;
#[cfg(test)]
use library_write::with_count_delta;
#[cfg(test)]
use library_write::without_item;
use navigation::finish_artist_load;
use navigation::navigate;
#[cfg(test)]
use playback::NextStep;
use playback::apply_player_event;
#[cfg(test)]
use playback::decide_next_step;
use playback::load_or_stop;
use playback::play_context;
use playback::prefetch_next;
use playback::set_volume;
#[cfg(test)]
use playback::should_hover_prefetch;

pub fn update(state: &mut State, action: Action, random_below: RandomBelow) -> Vec<Effect> {
    let generation = state.playback_generation;
    let lyrics_request = state.lyrics.request_id;
    let lyrics_open = state.lyrics.open;
    let mut effects = update_inner(state, action, random_below);
    if effects.iter().any(|e| {
        matches!(
            e,
            Effect::Player(PlayerCommand::Load(_) | PlayerCommand::Stop)
        )
    }) {
        state.playback_generation = generation.wrapping_add(1);
    }
    state.lyrics.request_id = state.lyrics.request_id.max(lyrics_request);
    if lyrics_open
        && state.page != Page::NowPlaying
        && !state.lyrics.open
        && !effects
            .iter()
            .any(|e| matches!(e, Effect::FetchLyrics(None)))
    {
        effects.push(Effect::FetchLyrics(None));
    }
    effects.extend(super::lyrics::sync(state));
    effects
}
fn update_inner(state: &mut State, action: Action, random_below: RandomBelow) -> Vec<Effect> {
    match action {
        action @ (Action::LyricsToggled
        | Action::LyricsDelaySet { .. }
        | Action::LyricsReloadRequested
        | Action::LyricsLoaded { .. }) => super::lyrics::apply(state, action),
        Action::ForPlayback { generation, event } => {
            if generation != state.playback_generation {
                return vec![];
            }
            apply_player_event(state, event)
        }
        Action::ForSession { generation, action } => {
            if generation != state.session_generation {
                return vec![];
            }
            update_inner(state, *action, random_below)
        }
        action @ (Action::DiscoveryOpened(_)
        | Action::DiscoveryRequested { .. }
        | Action::DiscoveryLoaded { .. }) => super::discovery::apply(state, action),
        Action::NoticeDismissed(index) => {
            if index < state.notices.len() {
                state.notices.remove(index);
            }
            vec![]
        }
        Action::NoticePosted(message) => {
            state.notices.push(message);
            vec![]
        }
        Action::Player(event) => apply_player_event(state, event),
        action @ (Action::OAuthTokenStored
        | Action::SignInCancelled
        | Action::SignInRetryRequested
        | Action::SessionExpired
        | Action::SignOutRequested
        | Action::OAuthClientIdChanged(..)
        | Action::OAuthClientSecretChanged(..)
        | Action::OAuthStartRequested
        | Action::OAuthUrlReady(..)
        | Action::AuthVerified(..)
        | Action::StoredAuthFound(..)) => auth::apply(state, action),
        action @ (Action::NavigatedTo(..)
        | Action::SearchInputChanged(..)
        | Action::SearchSubmitted
        | Action::SearchLoaded(..)
        | Action::PlaylistOpened(..)
        | Action::ArtistOpened(..)
        | Action::ArtistSearchRequested(..)
        | Action::ArtistLinkOpened(..)
        | Action::AlbumOpened(..)
        | Action::BackPressed
        | Action::NowPlayingOpened
        | Action::DiscoveryShelfOpened(..)) => navigation::apply(state, action),
        action @ (Action::HistoryRequested { .. }
        | Action::HistoryLoaded { .. }
        | Action::LibraryRefreshRequested
        | Action::PlaylistsLoaded(..)
        | Action::PlaylistCoversLoaded(..)
        | Action::LikedLoaded(..)
        | Action::PlaylistTracksLoaded(..)
        | Action::ArtistLoaded(..)
        | Action::AlbumLoaded(..)
        | Action::LikedPageLoaded { .. }
        | Action::PlaylistTracksPageLoaded { .. }
        | Action::LibraryCacheLoaded { .. }
        | Action::PlaylistTracksCacheLoaded(..)
        | Action::DiscoveryCacheLoaded(..)) => library::apply(state, action),
        action @ (Action::TrackLikeToggled(..)
        | Action::TrackAddedToPlaylist { .. }
        | Action::TrackRemovedFromPlaylist { .. }
        | Action::PlaylistCreateRequested(..)
        | Action::CreatePlaylistDialogOpened(..)
        | Action::CreatePlaylistDraftChanged(..)
        | Action::DialogDismissed
        | Action::PlaylistItemAdded { .. }
        | Action::PlaylistCreated(..)
        | Action::LibraryWriteFinished { .. }) => library_write::apply(state, action),
        action @ (Action::LikedShuffleRequested
        | Action::ContextPlayed { .. }
        | Action::TrackHovered(..)
        | Action::PlayToggled
        | Action::PlaybackRetryRequested
        | Action::NextPressed
        | Action::PreviousPressed
        | Action::SeekRequested(..)
        | Action::SessionRestored(..)
        | Action::RadioLoaded(..)) => playback::apply(state, action, random_below),
        action @ (Action::QueueRemoved(..)
        | Action::TrackPlayNext(..)
        | Action::TrackQueued(..)
        | Action::QueueJumped(..)
        | Action::QueueSelectionRemoved(..)
        | Action::QueueSelectionMoved { .. }
        | Action::QueueCleared
        | Action::ShuffleToggled
        | Action::RepeatCycled
        | Action::AutoplayToggled
        | Action::QueuePanelToggled) => queue_controls::apply(state, action, random_below),
        action @ (Action::VolumeSet(..)
        | Action::BalanceSet(..)
        | Action::EqualizerChanged(..)
        | Action::EqualizerPresetSaved(..)
        | Action::EqualizerPresetDeleted(..)) => audio_controls::apply(state, action),
        action @ (Action::RadioStartCancelled
        | Action::RadioStartRequested(..)
        | Action::RadioStarted { .. }) => radio::apply(state, action, random_below),
        action @ (Action::WinampToggled
        | Action::WinampScaleSet(..)
        | Action::WinampOnTopToggled
        | Action::SkinBrowserToggled
        | Action::SkinChosen(..)
        | Action::SkinFileDropped(..)
        | Action::SkinLoaded(..)
        | Action::SkinInstalled(..)
        | Action::SkinListRefreshed(..)) => skins::apply(state, action),
    }
}
fn set_loadable<T>(slot: &mut Loadable<T>, result: Result<T, String>) {
    *slot = match result {
        Ok(value) => Loadable::Loaded(value),
        Err(message) => Loadable::Failed(message),
    };
}
