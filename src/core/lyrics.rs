//! Current-track lyrics, with request identity independent of playback seeks.
use super::{
    action::Action,
    effect::Effect,
    model::{Track, TrackId},
    state::{Loadable, State},
};
#[derive(Clone, Debug, PartialEq)]
pub struct Lyrics {
    pub text: String,
    pub source: String,
    pub timed_lines: Vec<TimedLine>,
}
/// A line on the recording's playback timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct TimedLine {
    pub at: std::time::Duration,
    pub text: String,
}

/// Parse standard LRC, including repeated timestamps and the millisecond offset tag.
/// Untimed metadata is ignored; simultaneous lines are combined in source order.
pub fn parse_lrc(input: &str) -> Vec<TimedLine> {
    let offset = parse_offset(input);
    let mut entries: Vec<_> = input
        .lines()
        .flat_map(|line| parse_lrc_line(line, offset))
        .collect();
    entries.sort_by_key(|line| line.at);
    let mut lines = merge_simultaneous_lines(entries);
    if !lines.iter().any(|line| !line.text.is_empty()) {
        lines.clear();
    }
    lines
}

fn parse_offset(input: &str) -> i64 {
    input
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("[offset:")?
                .strip_suffix(']')?
                .parse::<i64>()
                .ok()
        })
        .next_back()
        .unwrap_or(0)
}

fn parse_lrc_line(line: &str, offset: i64) -> Vec<TimedLine> {
    let mut rest = line.trim();
    let mut stamps = Vec::new();
    while let Some(tagged) = rest.strip_prefix('[') {
        let Some((tag, tail)) = tagged.split_once(']') else {
            break;
        };
        if let Some(ms) = parse_timestamp(tag) {
            stamps.push(ms);
        }
        rest = tail;
    }
    let text = rest.trim().to_owned();
    stamps
        .into_iter()
        .map(|ms| TimedLine {
            at: std::time::Duration::from_secs_f64(((ms + offset as f64) / 1000.0).max(0.0)),
            text: text.clone(),
        })
        .collect()
}

fn parse_timestamp(tag: &str) -> Option<f64> {
    let (minutes, seconds) = tag.split_once(':')?;
    let minutes = minutes.parse::<u32>().ok()?;
    let seconds = seconds.parse::<f64>().ok()?;
    (0.0..60.0)
        .contains(&seconds)
        .then_some((f64::from(minutes) * 60.0 + seconds) * 1000.0)
}

fn merge_simultaneous_lines(entries: Vec<TimedLine>) -> Vec<TimedLine> {
    let mut lines: Vec<TimedLine> = Vec::new();
    for entry in entries {
        if let Some(last) = lines.last_mut().filter(|last| last.at == entry.at) {
            append_line_text(&mut last.text, &entry.text);
        } else {
            lines.push(entry);
        }
    }
    lines
}

fn append_line_text(target: &mut String, addition: &str) {
    if addition.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push('\n');
    }
    target.push_str(addition);
}

impl Lyrics {
    /// Positive delay makes lyrics appear later. Uses audio position, so pauses and seeks
    /// need no separate timer or synchronization state.
    pub fn active_line(&self, position: std::time::Duration, delay_seconds: f64) -> Option<usize> {
        let time = position.as_secs_f64() - delay_seconds;
        self.timed_lines
            .partition_point(|line| line.at.as_secs_f64() <= time)
            .checked_sub(1)
    }
}

#[derive(Clone, Debug, Default)]
pub struct LyricsState {
    pub open: bool,
    pub delays: std::collections::BTreeMap<String, f64>,
    pub track: Option<TrackId>,
    pub request_id: u64,
    pub content: Loadable<Option<Lyrics>>,
}
pub fn sync(state: &mut State) -> Vec<Effect> {
    if let Some(effects) = cancel_closed_request(state) {
        return effects;
    }
    let current = state.playback.queue.current().cloned();
    let track = current.as_ref().map(|t| t.id.clone());
    if same_lyrics_request(state, &track) {
        return vec![];
    }
    if track.is_none() && state.lyrics.track.is_none() {
        return vec![];
    }
    state.lyrics.request_id = state.lyrics.request_id.wrapping_add(1);
    state.lyrics.track = track.clone();
    state.lyrics.content = if track.is_some() {
        Loadable::Loading
    } else {
        Loadable::NotAsked
    };
    lyrics_effect(state, current, track)
}

fn cancel_closed_request(state: &mut State) -> Option<Vec<Effect>> {
    if state.lyrics.open || state.page == super::state::Page::NowPlaying {
        return None;
    }
    if matches!(state.lyrics.content, Loadable::Loading) {
        state.lyrics.request_id = state.lyrics.request_id.wrapping_add(1);
        state.lyrics.content = Loadable::NotAsked;
        return Some(vec![Effect::FetchLyrics(None)]);
    }
    Some(vec![])
}

fn same_lyrics_request(state: &State, track: &Option<TrackId>) -> bool {
    track == &state.lyrics.track && !matches!(state.lyrics.content, Loadable::NotAsked)
}

fn lyrics_effect(state: &State, current: Option<Track>, track: Option<TrackId>) -> Vec<Effect> {
    match current.and_then(|track| {
        let path = track.local_path()?.to_owned();
        Some((track, path))
    }) {
        Some((track, path)) => vec![Effect::FetchLocalLyrics {
            request_id: state.lyrics.request_id,
            track: track.id,
            path,
        }],
        None => vec![Effect::FetchLyrics(
            track.map(|id| (state.lyrics.request_id, id)),
        )],
    }
}
pub fn apply(state: &mut State, action: Action) -> Vec<Effect> {
    match action {
        Action::LyricsDelaySet { track, seconds } => apply_delay(state, track, seconds),
        Action::LyricsToggled => return toggle_lyrics(state),
        Action::LyricsReloadRequested => state.lyrics.content = Loadable::NotAsked,
        Action::LyricsLoaded {
            request_id,
            track,
            result,
        } => apply_loaded(state, request_id, track, result),
        _ => unreachable!("lyrics actions only"),
    }
    vec![]
}

fn apply_delay(state: &mut State, track: TrackId, seconds: f64) {
    if !seconds.is_finite() {
        return;
    }
    if seconds == 0.0 {
        state.lyrics.delays.remove(&track.0);
    } else {
        state
            .lyrics
            .delays
            .insert(track.0, seconds.clamp(-30.0, 30.0));
    }
}

fn toggle_lyrics(state: &mut State) -> Vec<Effect> {
    state.lyrics.open = !state.lyrics.open;
    if state.lyrics.open || state.page == super::state::Page::NowPlaying {
        return vec![];
    }
    state.lyrics.request_id = state.lyrics.request_id.wrapping_add(1);
    if matches!(state.lyrics.content, Loadable::Loading) {
        state.lyrics.content = Loadable::NotAsked;
    }
    vec![Effect::FetchLyrics(None)]
}

fn apply_loaded(
    state: &mut State,
    request_id: u64,
    track: TrackId,
    result: Result<Option<Lyrics>, String>,
) {
    let is_open = state.lyrics.open || state.page == super::state::Page::NowPlaying;
    let is_current =
        request_id == state.lyrics.request_id && state.lyrics.track.as_ref() == Some(&track);
    if !is_open || !is_current {
        return;
    }
    state.lyrics.content = match result {
        Ok(lyrics) => Loadable::Loaded(lyrics),
        Err(error) => Loadable::Failed(error),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        model::{MediaSource, Track},
        update::update,
    };
    use std::path::PathBuf;
    fn track(id: &str) -> Track {
        Track {
            source: Default::default(),
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
    fn send(state: &mut State, action: Action) -> Vec<Effect> {
        update(state, action, &mut |_| 0)
    }

    #[test]
    fn local_tracks_request_same_name_local_lyrics() {
        let mut state = State::default();
        let mut local = track("song");
        local.source = MediaSource::LocalFile {
            path: PathBuf::from("/music/song.wav"),
        };
        send(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![local],
                start: 0,
            },
        );
        let effects = send(&mut state, Action::LyricsToggled);
        assert!(effects.iter().any(|effect| matches!(effect, Effect::FetchLocalLyrics { path, .. } if path == &PathBuf::from("/music/song.wav"))));
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::FetchLyrics(_)))
        );
    }
    #[test]
    fn lyrics_follow_track_ignore_old_results_and_cancel_when_closed() {
        let mut state = State::default();
        send(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("a"), track("b")],
                start: 0,
            },
        );
        assert!(matches!(state.lyrics.content, Loadable::NotAsked));
        let effects = send(&mut state, Action::LyricsToggled);
        let old = state.lyrics.request_id;
        assert!(effects.contains(&Effect::FetchLyrics(Some((old, TrackId("a".into()))))));
        send(&mut state, Action::NextPressed);
        send(
            &mut state,
            Action::LyricsLoaded {
                request_id: old,
                track: TrackId("a".into()),
                result: Ok(Some(Lyrics {
                    text: "old".into(),
                    source: "test".into(),
                    timed_lines: vec![],
                })),
            },
        );
        assert!(matches!(state.lyrics.content, Loadable::Loading));
        let current = state.lyrics.request_id;
        send(
            &mut state,
            Action::LyricsLoaded {
                request_id: current,
                track: TrackId("b".into()),
                result: Ok(None),
            },
        );
        assert_eq!(state.lyrics.content, Loadable::Loaded(None));
        let effects = send(&mut state, Action::LyricsToggled);
        assert!(effects.contains(&Effect::FetchLyrics(None)));
        send(
            &mut state,
            Action::LyricsLoaded {
                request_id: current,
                track: TrackId("b".into()),
                result: Err("late".into()),
            },
        );
        assert_eq!(state.lyrics.content, Loadable::Loaded(None));
    }
    #[test]
    fn retry_and_sign_out_invalidate_requests() {
        let mut state = State::default();
        send(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("a")],
                start: 0,
            },
        );
        send(&mut state, Action::LyricsToggled);
        let old = state.lyrics.request_id;
        send(
            &mut state,
            Action::LyricsLoaded {
                request_id: old,
                track: TrackId("a".into()),
                result: Err("offline".into()),
            },
        );
        assert!(matches!(state.lyrics.content, Loadable::Failed(_)));
        send(&mut state, Action::LyricsReloadRequested);
        assert!(state.lyrics.request_id > old);
        let pending = state.lyrics.request_id;
        assert!(send(&mut state, Action::SignOutRequested).contains(&Effect::FetchLyrics(None)));
        send(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("a")],
                start: 0,
            },
        );
        send(&mut state, Action::LyricsToggled);
        assert!(state.lyrics.request_id > pending);
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn parses_offsets_repeated_tags_and_simultaneous_lines() {
        let lines = parse_lrc(
            "[ar:Example]\n[offset:-500]\n[00:03.25][00:01.5]Again\n[00:01.500]Together\n[00:04.000]\n[00:99]bad\n[00:NaN]bad",
        );
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].at, Duration::from_secs(1));
        assert_eq!(lines[0].text, "Again\nTogether");
        assert_eq!(lines[1].at, Duration::from_millis(2750));
        assert!(lines[2].text.is_empty());
        assert!(parse_lrc("plain words\n[ar:metadata]").is_empty());
    }
    #[test]
    fn follows_audio_boundaries_pause_backward_seek_and_user_delay() {
        let lyrics = Lyrics {
            text: String::new(),
            source: String::new(),
            timed_lines: parse_lrc("[00:01]One\n[00:02]Two\n[00:03]"),
        };
        let at = |ms, delay| lyrics.active_line(Duration::from_millis(ms), delay);
        assert_eq!(at(999, 0.0), None);
        assert_eq!(at(1000, 0.0), Some(0));
        assert_eq!(at(2000, 0.0), Some(1));
        assert_eq!(at(2000, 0.0), Some(1)); // paused position stays fixed
        assert_eq!(at(1000, 0.0), Some(0)); // seek backward
        assert_eq!(at(2000, 1.0), Some(0));
        assert_eq!(at(0, -1.0), Some(0));
        assert_eq!(at(3000, 0.0), Some(2)); // instrumental gap
    }
}
