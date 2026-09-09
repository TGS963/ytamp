use super::{
    Action, ApiRequest, Duration, Effect, PlayStatus, PlayerCommand, PlayerEvent, Queue,
    RandomBelow, State, Track, TrackId,
};

const RESTART_THRESHOLD: Duration = Duration::from_secs(3);

pub(super) fn apply(state: &mut State, action: Action, random_below: RandomBelow) -> Vec<Effect> {
    match action {
        Action::RadioLoaded(id, result) => finish_radio_load(state, id, result),
        Action::LikedShuffleRequested => {
            let tracks = state.library.liked.loaded().cloned().unwrap_or_default();
            if tracks.is_empty() {
                return vec![];
            }
            let start = random_below(tracks.len());
            state.playback.queue.set_shuffle(true, random_below);
            play_context(state, tracks, start, random_below)
        }
        Action::ContextPlayed { tracks, start } => play_context(state, tracks, start, random_below),
        Action::TrackHovered(track) => hover_prefetch(state, track),
        Action::PlayToggled => toggle_play(state),
        Action::PlaybackRetryRequested => {
            if state.playback.error.is_some() {
                restart_current(state)
            } else {
                vec![]
            }
        }
        Action::NextPressed => advance_or_start_radio(state, |state| state.playback.queue.next()),
        Action::PreviousPressed => go_previous(state),
        Action::SeekRequested(position) => {
            let position = state
                .playback
                .track_duration
                .map_or(position, |duration| position.min(duration));
            state.playback.position = position;
            if state.playback.loading || state.playback.resume_position.is_some() {
                state.playback.resume_position = Some(position);
                vec![]
            } else {
                vec![Effect::Player(PlayerCommand::Seek(position))]
            }
        }
        Action::SessionRestored(session) => restore_session(state, *session),
        _ => unreachable!("action routed to the wrong reducer domain"),
    }
}

pub(super) fn play_context(
    state: &mut State,
    tracks: Vec<Track>,
    start: usize,
    random_below: RandomBelow,
) -> Vec<Effect> {
    state.playback.radio_request = None;
    let started = state
        .playback
        .queue
        .play_context(tracks, start, random_below);
    load_track(state, started)
}

pub(super) fn toggle_play(state: &mut State) -> Vec<Effect> {
    match state.playback.status {
        PlayStatus::Playing => {
            state.playback.status = PlayStatus::Paused;
            vec![Effect::Player(PlayerCommand::Pause)]
        }
        PlayStatus::Paused => {
            state.playback.status = if state.playback.loading {
                PlayStatus::Loading
            } else {
                PlayStatus::Playing
            };
            vec![Effect::Player(PlayerCommand::Resume)]
        }
        PlayStatus::Stopped => restart_current(state),
        PlayStatus::Loading => {
            state.playback.status = PlayStatus::Paused;
            vec![Effect::Player(PlayerCommand::Pause)]
        }
    }
}

/// Play on a stopped player reloads the current track and continues
/// where it stopped. This is how a restored session resumes.
pub(super) fn restart_current(state: &mut State) -> Vec<Effect> {
    let Some(track) = state.playback.queue.current().cloned() else {
        return vec![];
    };
    let position = state.playback.position;
    let effects = load_track(state, Some(track));
    state.playback.resume_position = (!position.is_zero()).then_some(position);
    state.playback.position = position;
    effects
}

pub(super) fn restore_session(
    state: &mut State,
    session: crate::core::session::SavedSession,
) -> Vec<Effect> {
    state.lyrics.delays = session
        .lyrics_delays
        .clone()
        .into_iter()
        .filter(|(_, value)| value.is_finite() && *value != 0.0)
        .map(|(id, value)| (id, value.clamp(-30.0, 30.0)))
        .collect();
    state.equalizer = session.equalizer.clone().normalized();
    state.playback.position = session.position();
    state.playback.queue = session.queue;
    state.playback.volume = session.volume.clamp(0.0, 1.0);
    state.playback.balance = if session.balance.is_finite() {
        session.balance.clamp(-1., 1.)
    } else {
        0.
    };
    state.playback.autoplay = session.autoplay;
    state.playback.loading = false;
    state.playback.status = PlayStatus::Stopped;
    state.winamp = session.winamp;
    state.playback.track_duration = state
        .playback
        .queue
        .current()
        .and_then(|track| track.duration)
        .or(session
            .track_duration_secs
            .filter(|s| *s > 0)
            .map(Duration::from_secs));
    sync_queue_duration(state);
    vec![
        Effect::Player(PlayerCommand::SetVolume(state.playback.volume)),
        Effect::Player(PlayerCommand::SetBalance(state.playback.balance)),
        Effect::Player(PlayerCommand::SetEqualizer(state.equalizer.parameters)),
        Effect::LoadSkin(state.winamp.skin.clone()),
        Effect::RefreshSkinList,
    ]
}

pub(super) fn go_previous(state: &mut State) -> Vec<Effect> {
    if state.playback.position >= RESTART_THRESHOLD {
        state.playback.position = Duration::ZERO;
        return vec![Effect::Player(PlayerCommand::Seek(Duration::ZERO))];
    }
    load_or_stop(state, |state| state.playback.queue.previous())
}

pub(super) fn set_volume(state: &mut State, volume: f32) -> Vec<Effect> {
    let volume = volume.clamp(0.0, 1.0);
    state.playback.volume = volume;
    vec![Effect::Player(PlayerCommand::SetVolume(volume))]
}

fn sync_queue_duration(state: &mut State) {
    let id = state.playback.queue.current().map(|track| track.id.clone());
    if let (Some(id), Some(duration)) = (id, state.playback.track_duration) {
        state.playback.queue.set_duration(&id, duration);
    }
}

pub(super) fn apply_player_event(state: &mut State, event: PlayerEvent) -> Vec<Effect> {
    match event {
        PlayerEvent::TrackStarted {
            duration,
            channels,
            sample_rate,
        } => {
            state.playback.loading = false;
            if state.playback.status != PlayStatus::Paused {
                state.playback.status = PlayStatus::Playing;
            }
            state.playback.track_duration = duration.or(state.playback.track_duration);
            sync_queue_duration(state);
            state.playback.channels = channels;
            state.playback.sample_rate = sample_rate;
            let mut effects = prefetch_next(state);
            if let Some(position) = state.playback.resume_position.take() {
                state.playback.position = position;
                effects.push(Effect::Player(PlayerCommand::Seek(position)));
            }
            effects
        }
        PlayerEvent::PositionChanged(position) => {
            state.playback.position = position;
            vec![]
        }
        PlayerEvent::TrackEnded => {
            advance_or_start_radio(state, |state| state.playback.queue.on_track_end())
        }
        PlayerEvent::Failed(message) => {
            state.playback.loading = false;
            state.playback.status = PlayStatus::Stopped;
            state.playback.error = Some(message);
            vec![]
        }
    }
}

/// Advances the queue with `step` and loads the result, or stops playback
/// when the queue is exhausted.
pub(super) fn load_or_stop(
    state: &mut State,
    step: impl FnOnce(&mut State) -> Option<Track>,
) -> Vec<Effect> {
    let next = step(state);
    if next.is_none() {
        return stop_playback(state);
    }
    load_track(state, next)
}

/// Stops playback, unless it has already stopped.
pub(super) fn stop_playback(state: &mut State) -> Vec<Effect> {
    if state.playback.status == PlayStatus::Stopped {
        return vec![];
    }
    state.playback.loading = false;
    state.playback.status = PlayStatus::Stopped;
    vec![Effect::Player(PlayerCommand::Stop)]
}

/// What to do once the queue has no more tracks to hand back: stop,
/// or fetch a radio to keep playing, when autoplay is on and a track
/// was playing.
pub(super) enum NextStep {
    Load(Track),
    FetchRadio(TrackId),
    Stop,
}

/// Chooses the next step from the queue's answer, the autoplay
/// setting, and the track that was playing. Pure, so the reducer
/// logic here is unit-testable without a `State`.
pub(super) fn decide_next_step(
    next: Option<Track>,
    autoplay: bool,
    current: Option<&Track>,
) -> NextStep {
    match (next, autoplay, current) {
        (Some(track), _, _) => NextStep::Load(track),
        (None, true, Some(current)) => NextStep::FetchRadio(current.id.clone()),
        (None, _, _) => NextStep::Stop,
    }
}

/// Advances the queue with `step`. An empty queue with autoplay on
/// starts a radio fetch from the track that was playing, instead of
/// stopping at once.
pub(super) fn advance_or_start_radio(
    state: &mut State,
    step: impl FnOnce(&mut State) -> Option<Track>,
) -> Vec<Effect> {
    let current = state.playback.queue.current().cloned();
    let next = step(state);
    match decide_next_step(next, state.playback.autoplay, current.as_ref()) {
        NextStep::Load(track) => load_track(state, Some(track)),
        NextStep::FetchRadio(id) => start_radio_fetch(state, id),
        NextStep::Stop => stop_playback(state),
    }
}

/// Marks the player as loading and requests a radio for `track_id`,
/// remembering the request so a late or stale result can be told
/// apart from a fresh one.
pub(super) fn start_radio_fetch(state: &mut State, track_id: TrackId) -> Vec<Effect> {
    state.playback.error = None;
    state.playback.resume_position = None;
    state.playback.loading = true;
    state.playback.status = PlayStatus::Loading;
    state.playback.radio_request = Some(track_id.clone());
    vec![Effect::Api(ApiRequest::FetchRadio(track_id))]
}

/// Applies a radio result once it matches the running request. Drops
/// tracks already in the queue, then extends and advances, or stops
/// on an empty or failed radio.
pub(super) fn finish_radio_load(
    state: &mut State,
    track_id: TrackId,
    result: Result<Vec<Track>, String>,
) -> Vec<Effect> {
    if state.playback.radio_request != Some(track_id) {
        return vec![];
    }
    state.playback.radio_request = None;
    match result {
        Ok(tracks) => apply_radio_tracks(state, tracks),
        Err(message) => {
            state.notices.push(message);
            stop_playback(state)
        }
    }
}

/// Adds the radio tracks not already in the queue and plays the
/// first of them, or stops when none of them are new.
pub(super) fn apply_radio_tracks(state: &mut State, tracks: Vec<Track>) -> Vec<Effect> {
    let new_tracks = tracks_not_in(&state.playback.queue, tracks);
    if new_tracks.is_empty() {
        return stop_playback(state);
    }
    state.playback.queue.extend_context(new_tracks);
    load_or_stop(state, |state| state.playback.queue.next())
}

/// The tracks not already present in `queue`, in their given order.
pub(super) fn tracks_not_in(queue: &Queue, tracks: Vec<Track>) -> Vec<Track> {
    tracks
        .into_iter()
        .filter(|track| !queue.contains(&track.id))
        .collect()
}

pub(super) fn load_track(state: &mut State, track: Option<Track>) -> Vec<Effect> {
    let Some(track) = track else {
        return vec![];
    };
    state.playback.error = None;
    state.playback.resume_position = None;
    state.playback.loading = true;
    state.playback.status = PlayStatus::Loading;
    state.playback.position = Duration::ZERO;
    state.playback.track_duration = track.duration;
    vec![Effect::Player(PlayerCommand::Load(track))]
}

/// The effect that warms the cache for the track after the current
/// one, or no effect when the queue has nothing more to play.
pub(super) fn prefetch_next(state: &State) -> Vec<Effect> {
    vec![Effect::Player(PlayerCommand::PrepareNext(
        state.playback.queue.peek_next(),
    ))]
}

/// Warms the cache for a track the pointer has rested on, unless
/// `should_hover_prefetch` finds a reason to skip it. Remembers the
/// id, so a pointer that leaves and returns to the same row does not
/// start a second download.
pub(super) fn hover_prefetch(state: &mut State, track: Track) -> Vec<Effect> {
    let current = state.playback.queue.current().map(|track| &track.id);
    let last = state.playback.last_hover_prefetch.as_ref();
    if !should_hover_prefetch(current, last, &track.id) {
        return vec![];
    }
    state.playback.last_hover_prefetch = Some(track.id.clone());
    vec![Effect::Player(PlayerCommand::Prefetch(track))]
}

/// Whether a hover on `hovered` should start a prefetch: not the
/// track already playing, and not a repeat of the last hover
/// prefetch.
pub(super) fn should_hover_prefetch(
    current: Option<&TrackId>,
    last_hover_prefetch: Option<&TrackId>,
    hovered: &TrackId,
) -> bool {
    current != Some(hovered) && last_hover_prefetch != Some(hovered)
}
