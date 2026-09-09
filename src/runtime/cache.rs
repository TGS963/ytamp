use super::{Action, EffectRuntime, LibraryCacheWrite, PlaylistId};
use crate::library_cache;

impl EffectRuntime {
    /// Reads the cached playlist list and liked songs, and delivers
    /// them together even when one or both are a miss. The reducer
    /// decides what a miss means for each slot.
    pub(super) fn load_library_cache(&self) {
        let deliver = self.scoped_delivery();
        self.tokio.spawn_blocking(move || {
            if let Some(page) = library_cache::load_discovery() {
                deliver(Action::DiscoveryCacheLoaded(page));
            }
            let playlists = library_cache::load_playlists();
            let liked = library_cache::load_liked();
            deliver(Action::LibraryCacheLoaded { playlists, liked });
        });
    }

    /// Reads one playlist's cached track list. Delivers nothing on a
    /// miss, since a loading page already shows a spinner.
    pub(super) fn load_playlist_tracks_cache(&self, id: PlaylistId) {
        let deliver = self.scoped_delivery();
        self.tokio.spawn_blocking(move || {
            if let Some(tracks) = library_cache::load_playlist_tracks(&id) {
                deliver(Action::PlaylistTracksCacheLoaded(id, tracks));
            }
        });
    }

    /// Writes one piece of fresh network data to the library cache.
    /// The reducer has already applied it to the state by the time
    /// this effect runs.
    pub(super) fn save_library_cache(&self, write: LibraryCacheWrite) {
        let api = self.api.clone();
        let generation = api.read().expect("api lock").generation;
        self.tokio.spawn_blocking(move || {
            let guard = api.read().expect("api lock");
            if guard.generation != generation {
                return;
            }
            match write {
                LibraryCacheWrite::Discovery(page) => library_cache::save_discovery(&page),
                LibraryCacheWrite::Playlists(p) => library_cache::save_playlists(&p),
                LibraryCacheWrite::Liked(t) => library_cache::save_liked(&t),
                LibraryCacheWrite::PlaylistTracks(id, t) => {
                    library_cache::save_playlist_tracks(&id, &t)
                }
            }
        });
    }

    pub(super) fn clear_library_cache(&self) {
        library_cache::clear();
    }
}
