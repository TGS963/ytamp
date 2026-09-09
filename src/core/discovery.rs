//! Account-scoped, paginated YouTube discovery and collection navigation.
use super::{
    action::Action,
    effect::{ApiRequest, Effect},
    model::Track,
};
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Target {
    Browse {
        id: String,
        params: Option<String>,
    },
    Watch {
        video: Option<String>,
        playlist: Option<String>,
        params: Option<String>,
    },
}
impl Target {
    pub fn home() -> Self {
        Self::Browse {
            id: "FEtopics_music".into(),
            params: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub title: String,
    pub subtitle: String,
    pub artwork: Option<String>,
    pub target: Target,
    pub track: Option<Track>,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Shelf {
    pub title: String,
    pub entries: Vec<Entry>,
}
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FeedPage {
    pub shelves: Vec<Shelf>,
    pub tracks: Vec<Track>,
    pub continuation: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub struct Feed {
    pub page: FeedPage,
    pub target: Option<Target>,
    pub loaded: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub request_id: u64,
    pub appending: bool,
}
impl Feed {
    pub fn request(&mut self, target: Target, more: bool) -> Vec<Effect> {
        let changed = self.target.as_ref() != Some(&target);
        if !changed && (self.loading || (more && self.page.continuation.is_none())) {
            return vec![];
        }
        if changed {
            self.page = FeedPage::default();
            self.loaded = false;
        }
        self.target = Some(target.clone());
        self.loading = true;
        self.error = None;
        self.appending = more && !changed;
        self.request_id = self.request_id.wrapping_add(1);
        vec![Effect::Api(ApiRequest::FetchDiscovery {
            request_id: self.request_id,
            target,
            continuation: if self.appending {
                self.page.continuation.clone()
            } else {
                None
            },
        })]
    }
    pub fn apply(&mut self, request_id: u64, target: &Target, result: Result<FeedPage, String>) {
        if !self.loading || self.request_id != request_id || self.target.as_ref() != Some(target) {
            return;
        }
        self.loading = false;
        match result {
            Ok(mut page) => {
                if self.appending {
                    page.continuation = page
                        .continuation
                        .filter(|c| Some(c) != self.page.continuation.as_ref());
                    self.page.shelves.extend(page.shelves);
                    for track in page.tracks {
                        if !self.page.tracks.iter().any(|t| t.id == track.id) {
                            self.page.tracks.push(track);
                        }
                    }
                    self.page.continuation = page.continuation;
                } else {
                    self.page = page;
                }
                self.loaded = true;
            }
            Err(error) => self.error = Some(error),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Discovery {
    pub radio_request_id: u64,
    pub radio_loading: bool,
    pub home: Feed,
    pub collection: Feed,
}
pub fn apply(state: &mut super::state::State, action: Action) -> Vec<Effect> {
    match action {
        Action::DiscoveryOpened(entry) => open_entry(state, entry),
        Action::DiscoveryRequested { more } => request_page(state, more),
        Action::DiscoveryLoaded {
            request_id,
            target,
            result,
        } => apply_loaded(state, request_id, target, result),
        _ => unreachable!(),
    }
}

fn open_entry(state: &mut super::state::State, entry: super::discovery::Entry) -> Vec<Effect> {
    if entry.target == Target::home() {
        return open_home(state);
    }
    open_collection(state, entry)
}

fn open_collection(state: &mut super::state::State, entry: super::discovery::Entry) -> Vec<Effect> {
    use super::state::Page;
    let page = Page::Discovery(Box::new(entry.clone()));
    if state.page == page {
        return vec![];
    }
    state.history.push(state.page.clone());
    state.page = page;
    state.discovery.collection.request(entry.target, false)
}

fn open_home(state: &mut super::state::State) -> Vec<Effect> {
    use super::state::Page;
    state.history.push(state.page.clone());
    state.page = Page::Home;
    if state.discovery.home.loaded {
        vec![]
    } else {
        state.discovery.home.request(Target::home(), false)
    }
}

fn request_page(state: &mut super::state::State, more: bool) -> Vec<Effect> {
    use super::state::Page;
    match &state.page {
        Page::Discovery(entry) => state
            .discovery
            .collection
            .request(entry.target.clone(), more),
        _ => state.discovery.home.request(Target::home(), more),
    }
}

fn apply_loaded(
    state: &mut super::state::State,
    request_id: u64,
    target: Target,
    result: Result<FeedPage, String>,
) -> Vec<Effect> {
    let feed = select_feed(state, &target);
    let accepted = accepts_result(feed, request_id, &target, result.is_ok());
    let prefetch_more = accepted && !feed.appending;
    feed.apply(request_id, &target, result);
    let mut effects = cache_effect(accepted, &target, feed);
    if should_prefetch(feed, &target, prefetch_more) {
        effects.extend(feed.request(Target::home(), true));
    }
    effects
}

fn select_feed<'a>(state: &'a mut super::state::State, target: &Target) -> &'a mut Feed {
    if *target == Target::home() {
        &mut state.discovery.home
    } else {
        &mut state.discovery.collection
    }
}

fn cache_effect(accepted: bool, target: &Target, feed: &Feed) -> Vec<Effect> {
    if accepted && *target == Target::home() {
        vec![Effect::SaveLibraryCache(
            super::effect::LibraryCacheWrite::Discovery(feed.page.clone()),
        )]
    } else {
        vec![]
    }
}

fn accepts_result(feed: &Feed, request_id: u64, target: &Target, succeeded: bool) -> bool {
    succeeded
        && feed.loading
        && feed.request_id == request_id
        && feed.target.as_ref() == Some(target)
}

fn should_prefetch(feed: &Feed, target: &Target, requested: bool) -> bool {
    requested
        && *target == Target::home()
        && feed.loaded
        && !feed.loading
        && feed.page.shelves.len() < 6
        && feed.page.continuation.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_collections_reject_late_results_and_pagination_retries() {
        let mut feed = Feed::default();
        let a = Target::Browse {
            id: "A".into(),
            params: None,
        };
        let b = Target::Browse {
            id: "B".into(),
            params: None,
        };
        feed.request(a.clone(), false);
        feed.request(b.clone(), false);
        feed.apply(
            1,
            &a,
            Ok(FeedPage {
                continuation: Some("private".into()),
                ..Default::default()
            }),
        );
        assert!(feed.loading);
        assert!(feed.page.continuation.is_none());
        feed.apply(
            2,
            &b,
            Ok(FeedPage {
                continuation: Some("next".into()),
                ..Default::default()
            }),
        );
        feed.request(b.clone(), true);
        feed.apply(3, &b, Err("offline".into()));
        assert_eq!(feed.page.continuation.as_deref(), Some("next"));
        assert!(!feed.request(b.clone(), true).is_empty());
        feed.apply(
            4,
            &b,
            Ok(FeedPage {
                continuation: Some("next".into()),
                ..Default::default()
            }),
        );
        assert!(feed.page.continuation.is_none());
    }
    #[test]
    fn sign_out_discards_discovery_results() {
        use crate::core::{state::State, update::update};
        let mut state = State::default();
        let generation = state.session_generation;
        update(
            &mut state,
            Action::DiscoveryRequested { more: false },
            &mut |_| 0,
        );
        let request_id = state.discovery.home.request_id;
        update(&mut state, Action::SignOutRequested, &mut |_| 0);
        update(
            &mut state,
            Action::ForSession {
                generation,
                action: Box::new(Action::DiscoveryLoaded {
                    request_id,
                    target: Target::home(),
                    result: Ok(FeedPage {
                        continuation: Some("private".into()),
                        ..Default::default()
                    }),
                }),
            },
            &mut |_| 0,
        );
        assert!(!state.discovery.home.loaded);
        assert!(state.discovery.home.page.continuation.is_none());
    }
}

#[cfg(test)]
mod prefetch_tests {
    use super::*;
    #[test]
    fn home_prefetch_is_bounded_and_a_late_result_does_not_start_another_request() {
        let mut state = super::super::state::State::default();
        state.discovery.home.request(Target::home(), false);
        let complete = |request_id, cursor| Action::DiscoveryLoaded {
            request_id,
            target: Target::home(),
            result: Ok(FeedPage {
                continuation: Some(cursor),
                ..Default::default()
            }),
        };
        assert_eq!(
            apply(&mut state, complete(1, "a".into()))
                .iter()
                .filter(|e| matches!(e, Effect::Api(_)))
                .count(),
            1
        );
        assert!(
            apply(&mut state, complete(2, "b".into()))
                .iter()
                .all(|e| matches!(e, Effect::SaveLibraryCache(_)))
        );
        assert!(apply(&mut state, complete(1, "old".into())).is_empty());
        assert_eq!(state.discovery.home.page.continuation.as_deref(), Some("b"));
    }
}

#[cfg(test)]
mod controls_tests {
    use super::*;
    use crate::core::{
        model::TrackId,
        state::{Loadable, Page, State},
        update::update,
    };
    fn track(id: &str) -> Track {
        Track {
            id: TrackId(id.into()),
            title: id.into(),
            artists: vec![],
            album: None,
            album_id: None,
            duration: None,
            thumbnail_url: None,
            playlist_item_id: None,
        }
    }
    #[test]
    fn play_next_preserves_manually_reordered_queue() {
        let mut state = State::default();
        update(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("a"), track("b"), track("c")],
                start: 0,
            },
            &mut |_| 0,
        );
        update(&mut state, Action::TrackQueued(track("queued")), &mut |_| 0);
        state.playback.queue.move_upcoming(&[2], 0);
        let previous = state.playback.queue.upcoming().cloned().collect::<Vec<_>>();
        update(&mut state, Action::TrackPlayNext(track("next")), &mut |_| 0);
        assert_eq!(state.playback.queue.current().unwrap().id.0, "a");
        let actual = state.playback.queue.upcoming().cloned().collect::<Vec<_>>();
        assert_eq!(actual[0].id.0, "next");
        assert_eq!(actual[1..], previous);
    }
    #[test]
    fn radio_rejects_late_results_after_another_playback_choice() {
        let mut state = State::default();
        let effects = update(
            &mut state,
            Action::RadioStartRequested(track("seed")),
            &mut |_| 0,
        );
        let Effect::Api(ApiRequest::StartRadio {
            request_id,
            playback_generation,
            seed,
        }) = effects[0].clone()
        else {
            panic!()
        };
        update(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("chosen")],
                start: 0,
            },
            &mut |_| 0,
        );
        assert!(
            update(
                &mut state,
                Action::RadioStarted {
                    request_id,
                    playback_generation,
                    seed,
                    result: Ok(vec![track("radio")])
                },
                &mut |_| 0
            )
            .is_empty()
        );
        assert_eq!(state.playback.queue.current().unwrap().id.0, "chosen");
    }
    #[test]
    fn cache_is_immediate_but_never_overwrites_a_fresh_home_response() {
        let mut state = State::default();
        state.discovery.home.request(Target::home(), false);
        let cached = FeedPage {
            shelves: vec![Shelf {
                title: "cached".into(),
                entries: vec![],
            }],
            ..Default::default()
        };
        update(
            &mut state,
            Action::DiscoveryCacheLoaded(cached.clone()),
            &mut |_| 0,
        );
        assert!(state.discovery.home.loading && state.discovery.home.loaded);
        assert_eq!(state.discovery.home.page, cached);
        update(
            &mut state,
            Action::DiscoveryLoaded {
                request_id: 1,
                target: Target::home(),
                result: Ok(FeedPage::default()),
            },
            &mut |_| 0,
        );
        update(&mut state, Action::DiscoveryCacheLoaded(cached), &mut |_| 0);
        assert!(state.discovery.home.page.shelves.is_empty());
    }
    #[test]
    fn now_playing_lyrics_restart_after_leaving_during_load() {
        let mut state = State::default();
        update(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("a")],
                start: 0,
            },
            &mut |_| 0,
        );
        let first = update(&mut state, Action::NowPlayingOpened, &mut |_| 0);
        assert!(
            first
                .iter()
                .any(|e| matches!(e, Effect::FetchLyrics(Some(_))))
        );
        let id = state.lyrics.request_id;
        update(&mut state, Action::BackPressed, &mut |_| 0);
        assert!(matches!(state.lyrics.content, Loadable::NotAsked));
        update(&mut state, Action::NowPlayingOpened, &mut |_| 0);
        assert_eq!(state.page, Page::NowPlaying);
        assert!(state.lyrics.request_id > id);
        assert!(
            !state.lyrics.open,
            "embedded lyrics must not open a second window"
        );
    }
}
