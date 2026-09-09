//! Current-track lyrics, with request identity independent of playback seeks.
use super::{
    action::Action,
    effect::Effect,
    model::TrackId,
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
    let offset = input
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("[offset:")?
                .strip_suffix(']')?
                .parse::<i64>()
                .ok()
        })
        .next_back()
        .unwrap_or(0);
    let mut entries = Vec::new();
    for line in input.lines() {
        let mut rest = line.trim();
        let mut stamps = Vec::new();
        while let Some(tagged) = rest.strip_prefix('[') {
            let Some((tag, tail)) = tagged.split_once(']') else {
                break;
            };
            let timestamp = (|| {
                let (minutes, seconds) = tag.split_once(':')?;
                let minutes = minutes.parse::<u32>().ok()?;
                let seconds = seconds.parse::<f64>().ok()?;
                if !(0.0..60.0).contains(&seconds) {
                    return None;
                }
                Some((f64::from(minutes) * 60.0 + seconds) * 1000.0)
            })();
            if let Some(ms) = timestamp {
                stamps.push(ms);
            }
            rest = tail;
        }
        for ms in stamps {
            entries.push(TimedLine {
                at: std::time::Duration::from_secs_f64(((ms + offset as f64) / 1000.0).max(0.0)),
                text: rest.trim().to_owned(),
            });
        }
    }
    entries.sort_by_key(|line| line.at);
    let mut lines: Vec<TimedLine> = Vec::new();
    for entry in entries {
        if let Some(last) = lines.last_mut().filter(|last| last.at == entry.at) {
            if !entry.text.is_empty() {
                if !last.text.is_empty() {
                    last.text.push('\n');
                }
                last.text.push_str(&entry.text);
            }
        } else {
            lines.push(entry);
        }
    }
    if !lines.iter().any(|line| !line.text.is_empty()) {
        lines.clear();
    }
    lines
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
    if !state.lyrics.open && state.page != super::state::Page::NowPlaying {
        if matches!(state.lyrics.content, Loadable::Loading) {
            state.lyrics.request_id = state.lyrics.request_id.wrapping_add(1);
            state.lyrics.content = Loadable::NotAsked;
            return vec![Effect::FetchLyrics(None)];
        }
        return vec![];
    }
    let track = state.playback.queue.current().map(|t| t.id.clone());
    if track == state.lyrics.track && !matches!(state.lyrics.content, Loadable::NotAsked) {
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
    vec![Effect::FetchLyrics(
        track.map(|id| (state.lyrics.request_id, id)),
    )]
}
pub fn apply(state: &mut State, action: Action) -> Vec<Effect> {
    match action {
        Action::LyricsDelaySet { track, seconds } => {
            if seconds.is_finite() {
                if seconds == 0.0 {
                    state.lyrics.delays.remove(&track.0);
                } else {
                    state
                        .lyrics
                        .delays
                        .insert(track.0, seconds.clamp(-30.0, 30.0));
                }
            }
        }
        Action::LyricsToggled => {
            state.lyrics.open = !state.lyrics.open;
            if !state.lyrics.open && state.page != super::state::Page::NowPlaying {
                state.lyrics.request_id = state.lyrics.request_id.wrapping_add(1);
                if matches!(state.lyrics.content, Loadable::Loading) {
                    state.lyrics.content = Loadable::NotAsked;
                }
                return vec![Effect::FetchLyrics(None)];
            }
        }
        Action::LyricsReloadRequested => state.lyrics.content = Loadable::NotAsked,
        Action::LyricsLoaded {
            request_id,
            track,
            result,
        } => {
            if (state.lyrics.open || state.page == super::state::Page::NowPlaying)
                && request_id == state.lyrics.request_id
                && state.lyrics.track.as_ref() == Some(&track)
            {
                state.lyrics.content = match result {
                    Ok(lyrics) => Loadable::Loaded(lyrics),
                    Err(error) => Loadable::Failed(error),
                };
            }
        }
        _ => unreachable!("lyrics actions only"),
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{model::Track, update::update};
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
    fn send(state: &mut State, action: Action) -> Vec<Effect> {
        update(state, action, &mut |_| 0)
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
