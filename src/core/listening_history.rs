//! Account watch history, distinct from the back-navigation stack.
use super::{
    action::Action,
    effect::{ApiRequest, Effect},
    model::Track,
};
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryPage {
    pub tracks: Vec<Track>,
    pub continuation: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub struct ListeningHistory {
    pub tracks: Vec<Track>,
    pub continuation: Option<String>,
    pub loading: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub request_id: u64,
    pub appending: bool,
}
impl ListeningHistory {
    pub fn request(&mut self, more: bool) -> Vec<Effect> {
        if self.loading || (more && self.continuation.is_none()) {
            return vec![];
        }
        self.loading = true;
        self.error = None;
        self.appending = more;
        self.request_id = self.request_id.wrapping_add(1);
        vec![Effect::Api(ApiRequest::FetchHistory {
            request_id: self.request_id,
            continuation: if more {
                self.continuation.clone()
            } else {
                None
            },
        })]
    }
    pub fn apply(&mut self, action: Action) {
        let Action::HistoryLoaded { request_id, result } = action else {
            unreachable!()
        };
        if request_id != self.request_id || !self.loading {
            return;
        }
        self.loading = false;
        match result {
            Ok(page) => {
                if !self.appending {
                    self.tracks.clear();
                }
                self.tracks.extend(page.tracks);
                // A repeated cursor must not offer an endless Load older loop.
                self.continuation = page
                    .continuation
                    .filter(|next| !self.appending || Some(next) != self.continuation.as_ref());
                self.loaded = true;
            }
            Err(error) => self.error = Some(error),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sign_out_discards_pending_account_history() {
        use crate::core::{
            state::{Page, State},
            update::update,
        };
        let mut state = State::default();
        update(
            &mut state,
            Action::NavigatedTo(Page::ListeningHistory),
            &mut |_| 0,
        );
        let generation = state.session_generation;
        let request_id = state.listening_history.request_id;
        update(&mut state, Action::SignOutRequested, &mut |_| 0);
        update(
            &mut state,
            Action::ForSession {
                generation,
                action: Box::new(Action::HistoryLoaded {
                    request_id,
                    result: Ok(HistoryPage {
                        tracks: vec![],
                        continuation: Some("private-cursor".into()),
                    }),
                }),
            },
            &mut |_| 0,
        );
        assert!(!state.listening_history.loaded);
        assert!(state.listening_history.continuation.is_none());
    }
    #[test]
    fn stale_results_and_repeated_cursors_are_ignored_and_failures_retry() {
        let mut history = ListeningHistory::default();
        history.request(false);
        history.apply(Action::HistoryLoaded {
            request_id: 0,
            result: Err("late".into()),
        });
        assert!(history.loading);
        history.apply(Action::HistoryLoaded {
            request_id: 1,
            result: Ok(HistoryPage {
                tracks: vec![],
                continuation: Some("next".into()),
            }),
        });
        history.request(true);
        history.apply(Action::HistoryLoaded {
            request_id: 2,
            result: Err("offline".into()),
        });
        assert_eq!(history.continuation.as_deref(), Some("next"));
        assert!(!history.request(true).is_empty());
        history.apply(Action::HistoryLoaded {
            request_id: 3,
            result: Ok(HistoryPage {
                tracks: vec![],
                continuation: Some("next".into()),
            }),
        });
        assert!(history.continuation.is_none());
    }
}
