use super::{
    Action, AlbumId, ApiRequest, ArtistId, ArtistPage, ArtistRef, Effect, Loadable, Page,
    PlaylistId, State, fetch_missing_library, set_loadable, start_playlist_load,
};

pub(super) fn apply(state: &mut State, action: Action) -> Vec<Effect> {
    match action {
        Action::NavigatedTo(page) => navigate(state, page),
        Action::SearchInputChanged(input) => {
            state.search.input = input;
            vec![]
        }
        Action::SearchSubmitted => submit_search(state),
        Action::SearchLoaded(request_id, result) => {
            if request_id != state.search.request_id {
                return vec![];
            }
            set_loadable(&mut state.search.results, result);
            vec![]
        }
        Action::PlaylistOpened(id) => open_playlist(state, id),
        Action::ArtistOpened(id) => open_artist(state, id),
        Action::ArtistSearchRequested(name) => search_by_artist_name(state, name),
        Action::ArtistLinkOpened(artist) => open_artist_link(state, artist),
        Action::AlbumOpened(id) => open_album(state, id),
        Action::BackPressed => go_back(state),
        Action::NowPlayingOpened => {
            if state.page != Page::NowPlaying {
                state.history.push(state.page.clone());
                state.page = Page::NowPlaying;
            }
            vec![]
        }
        Action::DiscoveryShelfOpened(shelf) => {
            state.history.push(state.page.clone());
            state.page = Page::DiscoveryShelf(Box::new(shelf));
            vec![]
        }
        _ => unreachable!("action routed to the wrong reducer domain"),
    }
}

/// A sidebar jump to a whole section. Clears `history`: Back must
/// never cross a deliberate jump like this one.
pub(super) fn navigate(state: &mut State, page: Page) -> Vec<Effect> {
    state.history.clear();
    let effects = match page {
        Page::Home => {
            let mut effects = fetch_missing_library(state);
            if !state.discovery.home.loaded {
                effects.extend(
                    state
                        .discovery
                        .home
                        .request(crate::core::discovery::Target::home(), false),
                );
            }
            effects
        }
        Page::Library => fetch_missing_library(state),
        Page::ListeningHistory if !state.listening_history.loaded => {
            state.listening_history.request(false)
        }
        _ => vec![],
    };
    state.page = page;
    effects
}

pub(super) fn submit_search(state: &mut State) -> Vec<Effect> {
    let query = state.search.input.trim().to_string();
    if query.is_empty() {
        return vec![];
    }
    state.search.request_id += 1;
    state.search.results = Loadable::Loading;
    vec![Effect::Api(ApiRequest::Search {
        request_id: state.search.request_id,
        query,
    })]
}

/// A click on an artist row with no id: runs a name search in place
/// of opening the artist page directly.
pub(super) fn search_by_artist_name(state: &mut State, name: String) -> Vec<Effect> {
    push_history(state);
    show_search_for(state, name)
}

/// Opens the Search page with `name` as the query, in place of the
/// current page. The caller decides whether the history grows.
pub(super) fn show_search_for(state: &mut State, name: String) -> Vec<Effect> {
    state.search.input = name;
    state.page = Page::Search;
    submit_search(state)
}

/// An artist link carries a name and maybe an id. With an id the
/// artist page opens, and the name waits as the fallback for a
/// channel that turns out not to be an artist.
pub(super) fn open_artist_link(state: &mut State, artist: ArtistRef) -> Vec<Effect> {
    let Some(id) = artist.id else {
        return search_by_artist_name(state, artist.name);
    };
    let effects = open_artist(state, id);
    if !effects.is_empty() {
        state.browse.artist_fallback = Some(artist.name);
    }
    effects
}

/// Opens a playlist page and starts its two loads. A playlist already
/// open stays open, with no repeat fetch and no history entry.
pub(super) fn open_playlist(state: &mut State, id: PlaylistId) -> Vec<Effect> {
    let target = Page::Playlist(id.clone());
    if state.page == target {
        return vec![];
    }
    push_history(state);
    state.page = target;
    start_playlist_load(state, id)
}

/// Opens an artist page and starts its load. An artist page already
/// open stays open, with no repeat fetch and no history entry.
pub(super) fn open_artist(state: &mut State, id: ArtistId) -> Vec<Effect> {
    let target = Page::Artist(id.clone());
    if state.page == target {
        return vec![];
    }
    push_history(state);
    state.page = target;
    state.browse.artist = Loadable::Loading;
    vec![Effect::Api(ApiRequest::FetchArtist(id))]
}

/// Opens an album page and starts its load, the same way as
/// `open_artist`.
pub(super) fn open_album(state: &mut State, id: AlbumId) -> Vec<Effect> {
    let target = Page::Album(id.clone());
    if state.page == target {
        return vec![];
    }
    push_history(state);
    state.page = target;
    state.browse.album = Loadable::Loading;
    vec![Effect::Api(ApiRequest::FetchAlbum(id))]
}

/// Records the page the user is leaving, so Back can return to it.
pub(super) fn push_history(state: &mut State) {
    state.history.push(state.page.clone());
}

/// Returns to the page Back left. Does nothing with an empty history.
/// A playlist page always re-fetches its tracks, the simplest correct
/// rule since the track list carries no id of its own to check. An
/// artist or album page re-fetches only when its slot does not
/// already hold that same page.
pub(super) fn go_back(state: &mut State) -> Vec<Effect> {
    let Some(previous) = state.history.pop() else {
        return vec![];
    };
    state.page = previous.clone();
    match previous {
        Page::Discovery(entry) => state.discovery.collection.request(entry.target, false),
        Page::Playlist(id) => start_playlist_load(state, id),
        Page::Artist(id) => reopen_artist(state, id),
        Page::Album(id) => reopen_album(state, id),
        Page::SignIn
        | Page::Search
        | Page::Home
        | Page::NowPlaying
        | Page::DiscoveryShelf(_)
        | Page::Library
        | Page::ListeningHistory => vec![],
    }
}

pub(super) fn reopen_artist(state: &mut State, id: ArtistId) -> Vec<Effect> {
    if slot_holds(&state.browse.artist, |page| page.id == id) {
        return vec![];
    }
    state.browse.artist = Loadable::Loading;
    vec![Effect::Api(ApiRequest::FetchArtist(id))]
}

pub(super) fn reopen_album(state: &mut State, id: AlbumId) -> Vec<Effect> {
    if slot_holds(&state.browse.album, |page| page.album.id == id) {
        return vec![];
    }
    state.browse.album = Loadable::Loading;
    vec![Effect::Api(ApiRequest::FetchAlbum(id))]
}

/// Whether `slot` already holds a value that `holds` accepts. Used to
/// decide whether Back needs a fresh fetch for the page it restores.
pub(super) fn slot_holds<T>(slot: &Loadable<T>, holds: impl Fn(&T) -> bool) -> bool {
    match slot {
        Loadable::Loaded(value) | Loadable::Refreshing(value) => holds(value),
        _ => false,
    }
}

/// Applies a loaded or failed artist page. Ignores a result for an
/// artist page the user has already left, the same guard as
/// `finish_playlist_load`.
pub(super) fn finish_artist_load(
    state: &mut State,
    id: ArtistId,
    result: Result<ArtistPage, String>,
) -> Vec<Effect> {
    if state.page != Page::Artist(id) {
        return vec![];
    }
    let fallback = state.browse.artist_fallback.take();
    match (result, fallback) {
        (Err(message), Some(name)) => fall_back_to_search(state, name, message),
        (result, _) => {
            set_loadable(&mut state.browse.artist, result);
            vec![]
        }
    }
}

/// A channel with no artist page shows the search for its name in
/// place of an error page. The Search page replaces the artist page,
/// so Back still returns to where the click happened.
pub(super) fn fall_back_to_search(state: &mut State, name: String, message: String) -> Vec<Effect> {
    log::info!("no artist page for {name}: {message}");
    state.notices.push(format!(
        "No artist page for {name}. Showing search results."
    ));
    state.browse.artist = Loadable::NotAsked;
    show_search_for(state, name)
}
