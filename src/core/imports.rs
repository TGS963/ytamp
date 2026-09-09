//! Ordered local-file imports, independent of the signed-in account.
use super::{action::Action, effect::Effect, model::TrackId, queue::RandomBelow, state::State};
use std::{collections::VecDeque, path::PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Imports {
    pub generation: u64,
    next_id: u64,
    jobs: VecDeque<Job>,
    pub done: usize,
}
#[derive(Clone, Debug)]
struct Job {
    id: u64,
    paths: Vec<PathBuf>,
    replace: Option<TrackId>,
}
impl Imports {
    pub fn total(&self) -> usize {
        self.jobs.front().map_or(0, |job| job.paths.len())
    }
    pub fn pending(&self) -> bool {
        !self.jobs.is_empty()
    }
}
pub fn cancel(state: &mut State) -> Vec<Effect> {
    state.imports.generation = state.imports.generation.wrapping_add(1);
    state.imports.jobs.clear();
    state.imports.done = 0;
    vec![Effect::CancelLocalImport]
}
pub fn apply(state: &mut State, action: Action, random: RandomBelow) -> Vec<Effect> {
    match action {
        Action::LocalModeOpened => {
            open_local(state);
            vec![]
        }
        Action::YouTubeSignInOpened => {
            state.local_mode = false;
            vec![]
        }
        Action::AddFilesRequested => vec![Effect::PickLocalFiles {
            generation: state.imports.generation,
            replace: None,
        }],
        Action::LocalFileRemoveRequested(id) => {
            let current = state
                .playback
                .queue
                .current()
                .is_some_and(|track| track.id == id && track.is_local());
            state.playback.queue.remove_local(&id);
            if current {
                state.playback.error = None;
                state.playback.loading = false;
                state.playback.status = super::state::PlayStatus::Stopped;
                state.playback.track_duration = None;
                state.playback.position = std::time::Duration::ZERO;
                state.playback.resume_position = None;
                state.playback.channels = 0;
                state.playback.sample_rate = 0;
                return vec![Effect::Player(super::effect::PlayerCommand::Stop)];
            }
            vec![]
        }
        Action::LocalFileLocateRequested(id) => vec![Effect::PickLocalFiles {
            generation: state.imports.generation,
            replace: Some(id),
        }],
        Action::LocalFilesDropped(paths) => enqueue(state, paths, None),
        Action::LocalFilesChosen {
            generation,
            paths,
            replace,
        } => {
            if generation != state.imports.generation {
                return vec![];
            }
            enqueue(state, paths, replace)
        }
        Action::LocalImportProgress { id, done } => {
            if state.imports.jobs.front().is_some_and(|job| job.id == id) {
                state.imports.done = done;
            }
            vec![]
        }
        Action::LocalImportFinished { id, tracks, errors } => {
            complete(state, id, tracks, errors, random)
        }
        Action::LocalImportsCancelled => cancel(state),
        _ => unreachable!("local import action"),
    }
}
fn open_local(state: &mut State) {
    state.local_mode = true;
    state.page = super::state::Page::NowPlaying;
}
fn enqueue(state: &mut State, paths: Vec<PathBuf>, replace: Option<TrackId>) -> Vec<Effect> {
    if paths.is_empty() {
        return vec![];
    }
    if state.auth != super::state::AuthState::SignedIn {
        open_local(state);
    }
    state.imports.next_id = state.imports.next_id.wrapping_add(1);
    state.imports.jobs.push_back(Job {
        id: state.imports.next_id,
        paths,
        replace,
    });
    if state.imports.jobs.len() == 1 {
        start_next(state)
    } else {
        vec![]
    }
}
fn start_next(state: &mut State) -> Vec<Effect> {
    state.imports.done = 0;
    state
        .imports
        .jobs
        .front()
        .map(|job| Effect::ImportLocalFiles {
            id: job.id,
            paths: job.paths.clone(),
        })
        .into_iter()
        .collect()
}
fn complete(
    state: &mut State,
    id: u64,
    tracks: Vec<super::model::Track>,
    errors: Vec<String>,
    random: RandomBelow,
) -> Vec<Effect> {
    if state.imports.jobs.front().is_none_or(|job| job.id != id) {
        return vec![];
    }
    let job = state.imports.jobs.pop_front().expect("matching import");
    let count = tracks.len();
    let replacing = job.replace.is_some();
    let mut effects = match job.replace {
        Some(id) => replace(state, &id, tracks),
        None => super::update::playback::append_imported_tracks(state, tracks, random),
    };
    if count > 0 || !errors.is_empty() {
        let mut message = if replacing && count > 0 {
            "File location updated.".into()
        } else {
            format!("Added {count} file{}.", if count == 1 { "" } else { "s" })
        };
        if !errors.is_empty() {
            message.push_str(&format!(
                " {} could not be read:\n{}",
                errors.len(),
                errors.join("\n")
            ));
        }
        state.notices.push(message);
    }
    effects.extend(start_next(state));
    effects
}

fn replace(state: &mut State, id: &TrackId, tracks: Vec<super::model::Track>) -> Vec<Effect> {
    let Some(track) = tracks.into_iter().next() else {
        return vec![];
    };
    let current = state
        .playback
        .queue
        .current()
        .is_some_and(|t| &t.id == id && t.is_local());
    state.playback.failed_local.remove(id);
    state.playback.failed_local.remove(&track.id);
    state.playback.queue.replace_local(id, track.clone());
    if current {
        let position = state.playback.position;
        state.playback.track_duration = track.duration;
        state.playback.position = track
            .duration
            .map_or(position, |duration| position.min(duration));
        state.playback.error = None;
        state.playback.status = super::state::PlayStatus::Stopped;
        state.playback.loading = false;
        let mut effects = super::update::playback::restart_current(state);
        state.playback.status = super::state::PlayStatus::Paused;
        effects.push(Effect::Player(super::effect::PlayerCommand::Pause));
        return effects;
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::update::update;
    fn apply(state: &mut State, action: Action) -> Vec<Effect> {
        update(state, action, &mut |_| 0)
    }
    #[test]
    fn consecutive_drops_wait_their_turn_and_clear_invalidates_late_results() {
        let mut state = State::default();
        let first = apply(
            &mut state,
            Action::LocalFilesDropped(vec!["one.wav".into()]),
        );
        assert!(
            matches!(&first[..], [Effect::ImportLocalFiles { paths, .. }] if paths == &[PathBuf::from("one.wav")])
        );
        assert!(
            apply(
                &mut state,
                Action::LocalFilesDropped(vec!["two.wav".into()])
            )
            .is_empty()
        );
        assert!(state.imports.pending());
        let id = match first[0] {
            Effect::ImportLocalFiles { id, .. } => id,
            _ => unreachable!(),
        };
        let effects = apply(
            &mut state,
            Action::LocalImportFinished {
                id,
                tracks: vec![],
                errors: vec![],
            },
        );
        assert!(effects.iter().any(|e| matches!(e, Effect::ImportLocalFiles { paths, .. } if paths == &[PathBuf::from("two.wav")])));
        let effects = apply(&mut state, Action::QueueCleared);
        assert!(effects.contains(&Effect::CancelLocalImport));
        assert!(!state.imports.pending());
        apply(
            &mut state,
            Action::LocalImportFinished {
                id: id + 1,
                tracks: vec![],
                errors: vec!["late".into()],
            },
        );
        assert!(state.notices.is_empty());
    }
    fn track(name: &str) -> super::super::model::Track {
        use super::super::model::{MediaSource, Track, TrackId};
        Track {
            id: TrackId(format!("local:{name}")),
            source: MediaSource::LocalFile { path: name.into() },
            title: name.into(),
            artists: vec![],
            album: None,
            album_id: None,
            duration: Some(std::time::Duration::from_secs(90)),
            thumbnail_url: None,
            playlist_item_id: None,
        }
    }
    fn import_ready(state: &mut State, tracks: Vec<super::super::model::Track>) -> Vec<Effect> {
        let effects = apply(state, Action::LocalFilesDropped(vec!["test.wav".into()]));
        let id = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::ImportLocalFiles { id, .. } => Some(*id),
                _ => None,
            })
            .unwrap();
        apply(
            state,
            Action::LocalImportFinished {
                id,
                tracks,
                errors: vec![],
            },
        )
    }
    #[test]
    fn empty_queue_starts_first_and_keeps_duplicates_in_order() {
        let mut state = State::default();
        let effects = import_ready(&mut state, vec![track("one"), track("two"), track("two")]);
        assert_eq!(state.playback.queue.current().unwrap().title, "one");
        assert_eq!(
            state
                .playback
                .queue
                .upcoming()
                .map(|t| t.title.as_str())
                .collect::<Vec<_>>(),
            vec!["two", "two"]
        );
        assert!(effects.iter().any(|e| matches!(e, Effect::Player(super::super::effect::PlayerCommand::Load(t)) if t.title == "one")));
        assert!(state.local_mode);
    }
    #[test]
    fn dropping_during_paused_playback_appends_without_loading_or_reordering_context() {
        let mut state = State::default();
        apply(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("current"), track("context")],
                start: 0,
            },
        );
        apply(&mut state, Action::TrackQueued(track("queued")));
        state.playback.status = super::super::state::PlayStatus::Paused;
        let generation = state.playback_generation;
        let effects = import_ready(&mut state, vec![track("new")]);
        assert_eq!(
            state.playback.status,
            super::super::state::PlayStatus::Paused
        );
        assert_eq!(state.playback_generation, generation);
        assert_eq!(
            state
                .playback
                .queue
                .upcoming()
                .map(|t| t.title.as_str())
                .collect::<Vec<_>>(),
            vec!["queued", "new", "context"]
        );
        assert!(!effects.iter().any(|e| matches!(
            e,
            Effect::Player(super::super::effect::PlayerCommand::Load(_))
        )));
    }

    #[test]
    fn empty_paused_import_loads_first_then_pauses_and_preserves_order() {
        let mut state = State::default();
        state.playback.status = super::super::state::PlayStatus::Paused;
        let effects = import_ready(&mut state, vec![track("one"), track("two")]);
        assert_eq!(
            state.playback.status,
            super::super::state::PlayStatus::Paused
        );
        assert_eq!(state.playback.queue.current().unwrap().title, "one");
        assert_eq!(
            state
                .playback
                .queue
                .upcoming()
                .map(|track| track.title.as_str())
                .collect::<Vec<_>>(),
            vec!["two"]
        );
        assert!(matches!(
            effects.first(),
            Some(Effect::Player(super::super::effect::PlayerCommand::Load(track)))
                if track.title == "one"
        ));
        assert!(matches!(
            effects.get(1),
            Some(Effect::Player(super::super::effect::PlayerCommand::Pause))
        ));
        assert!(matches!(
            effects.get(2),
            Some(Effect::Player(super::super::effect::PlayerCommand::PrepareNext(Some(next))))
                if next.title == "two"
        ));
    }
    #[test]
    fn clearing_the_queue_invalidates_an_open_file_picker() {
        let mut state = State::default();
        let generation = state.imports.generation;
        apply(&mut state, Action::QueueCleared);
        let effects = apply(
            &mut state,
            Action::LocalFilesChosen {
                generation,
                replace: None,
                paths: vec!["late.wav".into()],
            },
        );
        assert!(effects.is_empty());
        assert!(!state.imports.pending());
    }
    #[test]
    fn locating_a_file_replaces_occurrences_and_preserves_order() {
        let mut state = State::default();
        import_ready(
            &mut state,
            vec![track("missing"), track("other"), track("missing")],
        );
        let generation = state.imports.generation;
        state.playback.position = std::time::Duration::from_secs(200);
        state.playback.failed_local.insert(track("missing").id);
        state.playback.failed_local.insert(track("found").id);
        let effects = apply(
            &mut state,
            Action::LocalFilesChosen {
                generation,
                replace: Some(track("missing").id),
                paths: vec!["found.wav".into()],
            },
        );
        let id = effects
            .iter()
            .find_map(|e| {
                if let Effect::ImportLocalFiles { id, .. } = e {
                    Some(*id)
                } else {
                    None
                }
            })
            .unwrap();
        let effects = apply(
            &mut state,
            Action::LocalImportFinished {
                id,
                tracks: vec![track("found")],
                errors: vec![],
            },
        );
        assert_eq!(state.playback.queue.current().unwrap().title, "found");
        assert_eq!(
            state
                .playback
                .queue
                .upcoming()
                .map(|t| t.title.as_str())
                .collect::<Vec<_>>(),
            vec!["other", "found"]
        );
        assert_eq!(
            state.playback.status,
            super::super::state::PlayStatus::Paused
        );
        assert_eq!(state.playback.position, std::time::Duration::from_secs(90));
        assert!(!state.playback.failed_local.contains(&track("missing").id));
        assert!(!state.playback.failed_local.contains(&track("found").id));
        assert!(matches!(
            effects.first(),
            Some(Effect::Player(super::super::effect::PlayerCommand::Load(track)))
                if track.title == "found"
        ));
        assert!(matches!(
            effects.get(1),
            Some(Effect::Player(super::super::effect::PlayerCommand::Pause))
        ));
        let effects = apply(&mut state, Action::PlayToggled);
        assert_eq!(
            effects,
            vec![Effect::Player(super::super::effect::PlayerCommand::Resume)]
        );
    }
}
