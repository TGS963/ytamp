//! The playback queue state machine.
//!
//! The rules, which the tests below enforce:
//!
//! - The context is the track list the user started playback from.
//! - The user queue holds explicitly queued tracks. It plays before the
//!   context advances, and its tracks never enter the context.
//! - Shuffle reorders only the tracks after the current one.
//! - An explicit Next always skips, also in repeat-one mode.
//! - A track end honors repeat-one and replays the same track.
//! - When the context ends in repeat-off mode, the current track stays
//!   visible and the caller stops playback.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::model::Track;

/// A source of random indices: `random_below(n)` returns a value in `0..n`.
///
/// The queue takes this as a parameter so its shuffle stays deterministic
/// in tests and the real randomness lives at the top level.
pub type RandomBelow<'a> = &'a mut dyn FnMut(usize) -> usize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn cycled(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackSource {
    Context,
    UserQueue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurrentTrack {
    pub track: Track,
    pub source: TrackSource,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Queue {
    context: Vec<Track>,
    /// The play order, as indices into `context`.
    order: Vec<usize>,
    /// The position of the current context track inside `order`.
    cursor: Option<usize>,
    user_queue: VecDeque<Track>,
    current: Option<CurrentTrack>,
    pub repeat: RepeatMode,
    pub shuffle: bool,
}

impl Queue {
    /// Replaces the context and starts playback at `start`.
    /// Returns the track to load, or `None` when `start` is out of range.
    pub fn play_context(
        &mut self,
        tracks: Vec<Track>,
        start: usize,
        random_below: RandomBelow,
    ) -> Option<Track> {
        let track = tracks.get(start)?.clone();
        let (order, cursor) = play_order(tracks.len(), start, self.shuffle, random_below);
        self.order = order;
        self.context = tracks;
        self.cursor = Some(cursor);
        self.current = Some(CurrentTrack {
            track: track.clone(),
            source: TrackSource::Context,
        });
        Some(track)
    }

    pub fn queue_track(&mut self, track: Track) {
        self.user_queue.push_back(track);
    }

    /// An explicit skip. Returns the track to load, or `None` when the
    /// queue is exhausted and playback stops.
    pub fn next(&mut self) -> Option<Track> {
        if let Some(track) = self.user_queue.pop_front() {
            self.current = Some(CurrentTrack {
                track: track.clone(),
                source: TrackSource::UserQueue,
            });
            return Some(track);
        }
        let cursor = self.advanced_cursor()?;
        self.cursor = Some(cursor);
        let track = self.context[self.order[cursor]].clone();
        self.current = Some(CurrentTrack {
            track: track.clone(),
            source: TrackSource::Context,
        });
        Some(track)
    }

    /// The track end signal. Returns the track to load next, or `None`
    /// when playback stops.
    pub fn on_track_end(&mut self) -> Option<Track> {
        if self.repeat == RepeatMode::One {
            return self.current.as_ref().map(|current| current.track.clone());
        }
        self.next()
    }

    /// Moves to the previous context track. Returns the track to load,
    /// or `None` when there is no current track.
    pub fn previous(&mut self) -> Option<Track> {
        let cursor = self.cursor?;
        let previous = self.previous_cursor(cursor);
        self.cursor = Some(previous);
        let track = self.context[self.order[previous]].clone();
        self.current = Some(CurrentTrack {
            track: track.clone(),
            source: TrackSource::Context,
        });
        Some(track)
    }

    /// Turns shuffle on or off and reorders the tracks after the current one.
    pub fn set_shuffle(&mut self, on: bool, random_below: RandomBelow) {
        self.shuffle = on;
        let Some(cursor) = self.cursor else {
            return;
        };
        let current_index = self.order[cursor];
        let (order, new_cursor) = play_order(self.context.len(), current_index, on, random_below);
        self.order = order;
        self.cursor = Some(new_cursor);
    }

    pub fn current(&self) -> Option<&Track> {
        self.current.as_ref().map(|current| &current.track)
    }

    /// The tracks that play after the current one: the user queue first,
    /// then the rest of the context in play order.
    pub fn upcoming(&self) -> impl Iterator<Item = &Track> {
        let context_rest = self
            .cursor
            .map(|cursor| &self.order[cursor + 1..])
            .unwrap_or_default()
            .iter()
            .map(|&index| &self.context[index]);
        self.user_queue.iter().chain(context_rest)
    }

    /// The track that would play after the current one, without
    /// changing state. Mirrors `on_track_end` and `next`, so a caller
    /// can prefetch it. In repeat-one mode, the next track is the
    /// current track.
    pub fn peek_next(&self) -> Option<Track> {
        if self.repeat == RepeatMode::One {
            return self.current().cloned();
        }
        if let Some(track) = self.user_queue.front() {
            return Some(track.clone());
        }
        let cursor = self.advanced_cursor()?;
        Some(self.context[self.order[cursor]].clone())
    }

    fn advanced_cursor(&self) -> Option<usize> {
        let next = self.cursor? + 1;
        if next < self.order.len() {
            return Some(next);
        }
        (self.repeat == RepeatMode::All).then_some(0)
    }

    fn previous_cursor(&self, cursor: usize) -> usize {
        if cursor > 0 {
            return cursor - 1;
        }
        match self.repeat {
            RepeatMode::All => self.order.len().saturating_sub(1),
            _ => 0,
        }
    }
}

/// The play order for a context of `len` tracks, and the cursor of the
/// `start` track inside it. Sequential order covers the whole context, so
/// Previous can reach the tracks before `start`. A shuffle puts `start`
/// first.
fn play_order(
    len: usize,
    start: usize,
    shuffle: bool,
    random_below: RandomBelow,
) -> (Vec<usize>, usize) {
    if !shuffle {
        return ((0..len).collect(), start);
    }
    let mut order: Vec<usize> = (0..len).collect();
    fisher_yates(&mut order, random_below);
    move_to_front(&mut order, start);
    (order, 0)
}

fn fisher_yates(values: &mut [usize], random_below: RandomBelow) {
    for i in (1..values.len()).rev() {
        values.swap(i, random_below(i + 1));
    }
}

fn move_to_front(order: &mut [usize], value: usize) {
    if let Some(position) = order.iter().position(|&index| index == value) {
        order[..=position].rotate_right(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::TrackId;

    fn track(id: &str) -> Track {
        Track {
            id: TrackId(id.to_string()),
            title: id.to_string(),
            artists: vec![],
            album: None,
            duration: None,
            thumbnail_url: None,
        }
    }

    fn tracks(ids: &[&str]) -> Vec<Track> {
        ids.iter().map(|id| track(id)).collect()
    }

    fn no_random(_: usize) -> usize {
        0
    }

    fn current_id(queue: &Queue) -> &str {
        &queue.current().expect("a current track").id.0
    }

    #[test]
    fn play_context_starts_at_the_given_track() {
        let mut queue = Queue::default();
        let started = queue.play_context(tracks(&["a", "b", "c"]), 1, &mut no_random);
        assert_eq!(started.unwrap().id.0, "b");
        assert_eq!(current_id(&queue), "b");
    }

    #[test]
    fn play_context_rejects_an_out_of_range_start() {
        let mut queue = Queue::default();
        assert_eq!(queue.play_context(tracks(&["a"]), 5, &mut no_random), None);
        assert_eq!(queue.current(), None);
    }

    #[test]
    fn next_walks_the_context_and_stops_at_the_end() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        assert_eq!(queue.next().unwrap().id.0, "b");
        assert_eq!(queue.next(), None);
        assert_eq!(current_id(&queue), "b");
    }

    #[test]
    fn next_plays_the_user_queue_before_the_context() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        queue.queue_track(track("q"));
        assert_eq!(queue.next().unwrap().id.0, "q");
        assert_eq!(queue.next().unwrap().id.0, "b");
    }

    #[test]
    fn repeat_all_wraps_at_the_end() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 1, &mut no_random);
        queue.repeat = RepeatMode::All;
        assert_eq!(queue.next().unwrap().id.0, "a");
    }

    #[test]
    fn repeat_one_replays_on_track_end_but_not_on_next() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        queue.repeat = RepeatMode::One;
        assert_eq!(queue.on_track_end().unwrap().id.0, "a");
        assert_eq!(queue.next().unwrap().id.0, "b");
    }

    #[test]
    fn previous_steps_back_and_stays_at_the_start() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 1, &mut no_random);
        assert_eq!(queue.previous().unwrap().id.0, "a");
        assert_eq!(queue.previous().unwrap().id.0, "a");
    }

    #[test]
    fn previous_returns_to_the_context_after_a_user_queue_track() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        queue.queue_track(track("q"));
        queue.next();
        assert_eq!(current_id(&queue), "q");
        assert_eq!(queue.previous().unwrap().id.0, "a");
    }

    #[test]
    fn shuffle_keeps_the_current_track_first() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b", "c", "d"]), 2, &mut no_random);
        let mut fake = |n: usize| n / 2;
        queue.set_shuffle(true, &mut fake);
        assert_eq!(current_id(&queue), "c");
        let upcoming: Vec<&str> = queue.upcoming().map(|t| t.id.0.as_str()).collect();
        assert_eq!(upcoming.len(), 3);
        assert!(!upcoming.contains(&"c"));
    }

    #[test]
    fn upcoming_lists_the_user_queue_then_the_context() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b", "c"]), 0, &mut no_random);
        queue.queue_track(track("q"));
        let upcoming: Vec<&str> = queue.upcoming().map(|t| t.id.0.as_str()).collect();
        assert_eq!(upcoming, vec!["q", "b", "c"]);
    }

    #[test]
    fn on_track_end_stops_in_repeat_off_at_the_context_end() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a"]), 0, &mut no_random);
        assert_eq!(queue.on_track_end(), None);
    }

    #[test]
    fn peek_next_reads_the_context_without_advancing() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        assert_eq!(queue.peek_next().unwrap().id.0, "b");
        assert_eq!(current_id(&queue), "a");
    }

    #[test]
    fn peek_next_prefers_the_user_queue() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        queue.queue_track(track("q"));
        assert_eq!(queue.peek_next().unwrap().id.0, "q");
    }

    #[test]
    fn peek_next_in_repeat_one_is_the_current_track() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        queue.repeat = RepeatMode::One;
        assert_eq!(queue.peek_next().unwrap().id.0, "a");
    }

    #[test]
    fn peek_next_is_none_at_the_context_end_in_repeat_off() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a"]), 0, &mut no_random);
        assert_eq!(queue.peek_next(), None);
    }
}
