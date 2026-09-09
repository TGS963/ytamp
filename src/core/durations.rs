//! Resolve missing queue metadata in bounded batches, independently of audio.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use super::effect::{ApiRequest, Effect};
use super::model::TrackId;
use super::state::{AuthState, State};

#[derive(Clone, Debug, Default)]
pub struct Lookup {
    attempts: HashMap<TrackId, u8>,
    pending: Vec<TrackId>,
    session: u64,
}

pub(super) fn sync(state: &mut State) -> Vec<Effect> {
    if state.auth != AuthState::SignedIn {
        return vec![];
    }
    let lookup = &mut state.playback.duration_lookup;
    if lookup.session != state.session_generation {
        *lookup = Lookup {
            session: state.session_generation,
            ..Default::default()
        };
    }
    if !lookup.pending.is_empty() {
        return vec![];
    }
    let mut seen = HashSet::new();
    let ids: Vec<_> = state
        .playback
        .queue
        .current()
        .into_iter()
        .chain(state.playback.queue.upcoming())
        .filter(|track| track.duration.is_none())
        .map(|track| track.id.clone())
        .filter(|id| {
            lookup.attempts.get(id).copied().unwrap_or_default() < 3 && seen.insert(id.clone())
        })
        .take(50)
        .collect();
    if ids.is_empty() {
        return vec![];
    }
    for id in &ids {
        *lookup.attempts.entry(id.clone()).or_default() += 1;
    }
    lookup.pending = ids.clone();
    vec![Effect::Api(ApiRequest::FetchTrackDurations(ids))]
}

pub(super) fn complete(state: &mut State, result: Result<Vec<(TrackId, Duration)>, String>) {
    let pending = std::mem::take(&mut state.playback.duration_lookup.pending);
    if let Ok(durations) = result {
        for (id, duration) in durations {
            if pending.contains(&id) && !duration.is_zero() {
                state.playback.queue.fill_missing_duration(&id, duration);
            }
        }
    }
    report_unresolved(state, &pending);
    if state.playback.track_duration.is_none() {
        state.playback.track_duration = state
            .playback
            .queue
            .current()
            .and_then(|track| track.duration);
    }
}

fn report_unresolved(state: &mut State, pending: &[TrackId]) {
    let unresolved = state
        .playback
        .queue
        .current()
        .into_iter()
        .chain(state.playback.queue.upcoming())
        .any(|track| {
            track.duration.is_none()
                && pending.contains(&track.id)
                && state.playback.duration_lookup.attempts.get(&track.id) == Some(&3)
        });
    if unresolved {
        state
            .notices
            .push("YouTube could not provide some song durations after three attempts.".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::action::{Action, PlayerEvent};
    use crate::core::effect::ApiRequest;
    use crate::core::model::Track;
    use crate::core::update::update;

    fn track(id: impl Into<String>) -> Track {
        let id = id.into();
        Track {
            id: TrackId(id.clone()),
            title: id,
            artists: vec![],
            album: None,
            album_id: None,
            duration: None,
            thumbnail_url: None,
            playlist_item_id: None,
        }
    }

    fn signed_in_state() -> State {
        State {
            auth: AuthState::SignedIn,
            session_generation: 1,
            ..State::default()
        }
    }

    fn apply(state: &mut State, action: Action) -> Vec<Effect> {
        update(state, action, &mut |_| 0)
    }

    fn duration_request(effects: &[Effect]) -> Option<Vec<TrackId>> {
        effects.iter().find_map(|effect| match effect {
            Effect::Api(ApiRequest::FetchTrackDurations(ids)) => Some(ids.clone()),
            _ => None,
        })
    }

    #[test]
    fn sync_batches_fifty_unique_missing_ids_and_skips_known_durations() {
        let mut state = signed_in_state();
        let mut tracks: Vec<_> = (0..55).map(|i| track(format!("missing-{i}"))).collect();
        tracks.push(track("missing-0"));
        let mut known = track("known");
        known.duration = Some(Duration::from_secs(99));
        tracks.push(known);

        let effects = apply(&mut state, Action::ContextPlayed { tracks, start: 0 });
        let ids = duration_request(&effects).expect("one duration request");

        let next = apply(
            &mut state,
            Action::TrackDurationsLoaded(Ok(ids
                .iter()
                .map(|id| (id.clone(), Duration::from_secs(123)))
                .collect())),
        );
        assert_eq!(
            duration_request(&next),
            Some((50..55).map(|i| TrackId(format!("missing-{i}"))).collect())
        );
        assert_eq!(ids.len(), 50);
        assert_eq!(
            ids,
            (0..50)
                .map(|i| TrackId(format!("missing-{i}")))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn metadata_result_hydrates_every_queued_clone_of_an_id() {
        let mut state = signed_in_state();
        apply(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("same"), track("other"), track("same")],
                start: 0,
            },
        );
        apply(&mut state, Action::TrackQueued(track("same")));

        apply(
            &mut state,
            Action::TrackDurationsLoaded(Ok(vec![(
                TrackId("same".into()),
                Duration::from_secs(123),
            )])),
        );

        let same_durations: Vec<_> = state
            .playback
            .queue
            .current()
            .into_iter()
            .chain(state.playback.queue.upcoming())
            .filter(|track| track.id == TrackId("same".into()))
            .map(|track| track.duration)
            .collect();
        assert_eq!(same_durations, vec![Some(Duration::from_secs(123)); 3]);
        let saved = crate::core::session::SavedSession::capture(&state);
        let restored =
            crate::core::session::SavedSession::from_json(&saved.to_json().unwrap()).unwrap();
        let mut restored_state = State::default();
        apply(
            &mut restored_state,
            Action::SessionRestored(Box::new(restored)),
        );
        assert_eq!(restored_state.playback.queue, state.playback.queue);
    }

    #[test]
    fn catalog_duration_cannot_replace_the_decoded_duration() {
        let mut state = signed_in_state();
        apply(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("current")],
                start: 0,
            },
        );
        apply(
            &mut state,
            Action::Player(PlayerEvent::TrackStarted {
                duration: Some(Duration::from_secs(240)),
                channels: 2,
                sample_rate: 44_100,
            }),
        );

        apply(
            &mut state,
            Action::TrackDurationsLoaded(Ok(vec![(
                TrackId("current".into()),
                Duration::from_secs(180),
            )])),
        );

        assert_eq!(
            state.playback.track_duration,
            Some(Duration::from_secs(240))
        );
        assert_eq!(
            state
                .playback
                .queue
                .current()
                .and_then(|track| track.duration),
            Some(Duration::from_secs(240))
        );
    }

    #[test]
    fn failed_lookups_retry_three_times_then_stop() {
        let mut state = signed_in_state();
        let effects = apply(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("missing")],
                start: 0,
            },
        );
        assert_eq!(
            duration_request(&effects),
            Some(vec![TrackId("missing".into())])
        );

        for attempt in 1..=3 {
            let effects = apply(
                &mut state,
                Action::TrackDurationsLoaded(Err("temporary failure".into())),
            );
            assert_eq!(
                duration_request(&effects),
                (attempt < 3).then(|| vec![TrackId("missing".into())]),
                "retry {attempt}"
            );
        }
        assert_eq!(
            state.notices,
            vec!["YouTube could not provide some song durations after three attempts."]
        );
    }

    #[test]
    fn stale_session_duration_result_cannot_hydrate_the_new_session_queue() {
        let mut state = signed_in_state();
        state.session_generation = 9;
        apply(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("current")],
                start: 0,
            },
        );

        let effects = apply(
            &mut state,
            Action::ForSession {
                generation: 8,
                action: Box::new(Action::TrackDurationsLoaded(Ok(vec![(
                    TrackId("current".into()),
                    Duration::from_secs(180),
                )]))),
            },
        );

        assert!(effects.is_empty());
        assert_eq!(
            state
                .playback
                .queue
                .current()
                .and_then(|track| track.duration),
            None
        );
        assert_eq!(state.playback.track_duration, None);
    }
}
