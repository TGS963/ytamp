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

use super::model::{Track, TrackId};

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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum UpcomingEntry {
    User(usize),
    Context(usize),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Queue {
    context: Vec<Track>,
    /// The play order, as indices into `context`.
    order: Vec<usize>,
    /// The position of the current context track inside `order`.
    cursor: Option<usize>,
    user_queue: VecDeque<Track>,
    #[serde(default)]
    manual_order: Option<Vec<UpcomingEntry>>,
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
        self.manual_order = None;
        self.order = order;
        self.context = tracks;
        self.cursor = Some(cursor);
        self.current = Some(CurrentTrack {
            track: track.clone(),
            source: TrackSource::Context,
        });
        Some(track)
    }

    fn entries(&self) -> Vec<UpcomingEntry> {
        self.manual_order.clone().unwrap_or_else(|| {
            (0..self.user_queue.len())
                .map(UpcomingEntry::User)
                .chain(
                    self.cursor
                        .into_iter()
                        .flat_map(|c| self.order[c + 1..].iter().copied())
                        .map(UpcomingEntry::Context),
                )
                .collect()
        })
    }
    fn entry_track(&self, entry: UpcomingEntry) -> Option<&Track> {
        match entry {
            UpcomingEntry::User(i) => self.user_queue.get(i),
            UpcomingEntry::Context(i) => self.context.get(i),
        }
    }
    /// Updates every queued occurrence without changing playback order.
    pub fn set_duration(&mut self, id: &TrackId, duration: std::time::Duration) {
        for track in self
            .context
            .iter_mut()
            .chain(self.user_queue.iter_mut())
            .chain(self.current.iter_mut().map(|current| &mut current.track))
            .filter(|track| &track.id == id)
        {
            track.duration = Some(duration);
        }
    }

    /// Adds catalog metadata without replacing a duration measured by the decoder.
    pub fn fill_missing_duration(&mut self, id: &TrackId, duration: std::time::Duration) {
        for track in self
            .context
            .iter_mut()
            .chain(self.user_queue.iter_mut())
            .chain(self.current.iter_mut().map(|current| &mut current.track))
            .filter(|track| &track.id == id && track.duration.is_none())
        {
            track.duration = Some(duration);
        }
    }

    /// Move selected upcoming occurrences before an insertion boundary in the
    /// original list. Keeps current playback, source identity and duplicates.
    pub fn move_upcoming(&mut self, selected: &[usize], before: usize) {
        let entries = self.entries();
        if before > entries.len() {
            return;
        }
        let selected: std::collections::BTreeSet<_> = selected
            .iter()
            .copied()
            .filter(|i| *i < entries.len())
            .collect();
        if selected.is_empty() {
            return;
        }
        let moving: Vec<_> = selected.iter().map(|i| entries[*i]).collect();
        let target = before - selected.iter().filter(|i| **i < before).count();
        let mut remaining: Vec<_> = entries
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !selected.contains(i))
            .map(|(_, e)| e)
            .collect();
        remaining.splice(target..target, moving);
        self.manual_order = Some(remaining);
    }
    /// Remove one upcoming occurrence without moving the current track.
    pub fn remove_upcoming(&mut self, index: usize) {
        let Some(entry) = self.entries().get(index).copied() else {
            return;
        };
        if let Some(order) = &mut self.manual_order {
            order.remove(index);
        }
        match entry {
            UpcomingEntry::User(i) => {
                self.user_queue.remove(i);
                self.adjust_removed(entry);
            }
            UpcomingEntry::Context(i) => {
                self.order.retain(|v| *v != i);
                self.context.remove(i);
                for v in &mut self.order {
                    if *v > i {
                        *v -= 1;
                    }
                }
                self.adjust_removed(entry);
            }
        }
    }
    fn adjust_removed(&mut self, removed: UpcomingEntry) {
        if let Some(order) = &mut self.manual_order {
            for entry in order {
                match (entry, removed) {
                    (UpcomingEntry::User(value), UpcomingEntry::User(i))
                    | (UpcomingEntry::Context(value), UpcomingEntry::Context(i))
                        if *value > i =>
                    {
                        *value -= 1
                    }
                    _ => {}
                }
            }
        }
    }
    pub fn play_next(&mut self, track: Track) {
        if let Some(order) = &mut self.manual_order {
            for entry in order.iter_mut() {
                if let UpcomingEntry::User(i) = entry {
                    *i += 1;
                }
            }
            order.insert(0, UpcomingEntry::User(0));
        }
        self.user_queue.push_front(track);
    }
    pub fn queue_track(&mut self, track: Track) {
        if let Some(order) = &mut self.manual_order {
            let at = order
                .iter()
                .rposition(|e| matches!(e, UpcomingEntry::User(_)))
                .map_or(0, |i| i + 1);
            order.insert(at, UpcomingEntry::User(self.user_queue.len()));
        }
        self.user_queue.push_back(track);
    }

    /// Drops every explicitly queued track, leaving the context alone.
    pub fn clear_user_queue(&mut self) {
        self.user_queue.clear();
        if let Some(order) = &mut self.manual_order {
            order.retain(|e| matches!(e, UpcomingEntry::Context(_)));
        }
    }

    /// Jumps straight to the track at `index` in [`Self::upcoming`],
    /// as if `next` had run enough times to reach it, skipping the
    /// tracks in between without playing them. Returns the track to
    /// load, or `None` when `index` is out of range.
    pub fn jump_to(&mut self, index: usize) -> Option<Track> {
        if index >= self.upcoming().count() {
            return None;
        }
        let mut track = None;
        for _ in 0..=index {
            track = self.next();
        }
        track
    }

    /// Appends tracks to the end of the context and the play order.
    /// A radio result lands here, after the queue has emptied.
    ///
    /// The new tracks join in list order, also when shuffle is on. A
    /// radio result already comes back in a listenable order, so no
    /// extra shuffle step runs on top of it.
    pub fn extend_context(&mut self, tracks: Vec<Track>) {
        let start = self.context.len();
        let new_indices = start..start + tracks.len();
        if let Some(order) = &mut self.manual_order {
            order.extend(new_indices.clone().map(UpcomingEntry::Context));
        }
        self.context.extend(tracks);
        self.order.extend(new_indices);
    }

    /// True when a track with this id already sits in the context.
    pub fn contains(&self, id: &TrackId) -> bool {
        self.context.iter().any(|track| &track.id == id)
    }

    pub fn replace_local(&mut self, id: &TrackId, replacement: Track) -> bool {
        if !replacement.is_local() {
            return false;
        }
        let mut replaced = false;
        for track in self
            .context
            .iter_mut()
            .chain(self.user_queue.iter_mut())
            .chain(self.current.iter_mut().map(|current| &mut current.track))
            .filter(|track| track.is_local() && &track.id == id)
        {
            *track = replacement.clone();
            replaced = true;
        }
        replaced
    }

    pub fn retain_local(&mut self) {
        self.retain_tracks(|track| track.is_local());
    }

    pub fn remove_local(&mut self, id: &TrackId) -> bool {
        let had_match = self
            .context
            .iter()
            .chain(self.user_queue.iter())
            .any(|track| track.is_local() && &track.id == id)
            || self
                .current
                .as_ref()
                .is_some_and(|current| current.track.is_local() && &current.track.id == id);
        self.retain_tracks(|track| !track.is_local() || &track.id != id);
        had_match
    }

    fn retain_tracks(&mut self, keep: impl Fn(&Track) -> bool) {
        let old_context = std::mem::take(&mut self.context);
        let mut context_map = vec![None; old_context.len()];
        let mut new_context = Vec::new();
        for (index, track) in old_context.into_iter().enumerate() {
            if keep(&track) {
                context_map[index] = Some(new_context.len());
                new_context.push(track);
            }
        }
        self.context = new_context;
        let old_order = std::mem::take(&mut self.order);
        let old_cursor = self.cursor;
        self.order = old_order
            .iter()
            .filter_map(|index| context_map.get(*index).copied().flatten())
            .collect();
        self.cursor = old_cursor.and_then(|cursor| {
            let retained_before = old_order
                .iter()
                .take(cursor)
                .filter_map(|index| context_map.get(*index).copied().flatten())
                .count();
            if old_order
                .get(cursor)
                .and_then(|index| context_map.get(*index))
                .is_some_and(Option::is_some)
            {
                Some(retained_before)
            } else {
                retained_before.checked_sub(1)
            }
        });
        let old_user = std::mem::take(&mut self.user_queue);
        let mut user_map = vec![None; old_user.len()];
        let mut new_user = VecDeque::new();
        for (index, track) in old_user.into_iter().enumerate() {
            if keep(&track) {
                user_map[index] = Some(new_user.len());
                new_user.push_back(track);
            }
        }
        self.user_queue = new_user;
        if let Some(order) = &mut self.manual_order {
            *order = remap_manual_order(order, &user_map, &context_map);
        }
        if self
            .current
            .as_ref()
            .is_some_and(|current| !keep(&current.track))
        {
            self.current = None;
        }
    }

    /// An explicit skip. Returns the track to load, or `None` when the
    /// queue is exhausted and playback stops.
    // A repeatable transport action, not an iterator (Previous can move back).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Track> {
        if let Some(entry) = self
            .manual_order
            .as_mut()
            .and_then(|order| (!order.is_empty()).then(|| order.remove(0)))
        {
            let (track, source) = self.take_manual_entry(entry)?;
            self.current = Some(CurrentTrack {
                track: track.clone(),
                source,
            });
            return Some(track);
        }
        self.manual_order = None;
        if let Some(track) = self.user_queue.pop_front() {
            self.current = Some(CurrentTrack {
                track: track.clone(),
                source: TrackSource::UserQueue,
            });
            return Some(track);
        }
        let cursor = self
            .advanced_cursor()
            .or_else(|| self.cursor.is_none().then_some(0))?;
        self.cursor = Some(cursor);
        let track = self.context[self.order[cursor]].clone();
        self.current = Some(CurrentTrack {
            track: track.clone(),
            source: TrackSource::Context,
        });
        Some(track)
    }

    fn take_manual_entry(&mut self, entry: UpcomingEntry) -> Option<(Track, TrackSource)> {
        let selected = match entry {
            UpcomingEntry::User(i) => {
                let track = self.user_queue.remove(i)?;
                self.adjust_removed(entry);
                (track, TrackSource::UserQueue)
            }
            UpcomingEntry::Context(i) => {
                let next = self.cursor.map_or(0, |cursor| cursor + 1);
                let position = self.order.iter().position(|v| *v == i)?;
                if position < next {
                    return None;
                }
                self.order[next..=position].rotate_right(1);
                self.cursor = Some(next);
                (self.context.get(i)?.clone(), TrackSource::Context)
            }
        };
        Some(selected)
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
        if previous > cursor {
            self.manual_order = None;
        } else if let Some(order) = &mut self.manual_order {
            let replay = self.order[previous + 1..cursor + 1]
                .iter()
                .copied()
                .map(UpcomingEntry::Context);
            order.splice(0..0, replay);
        }
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
        self.manual_order = None;
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
    pub fn upcoming(&self) -> Box<dyn Iterator<Item = &Track> + '_> {
        if let Some(order) = &self.manual_order {
            Box::new(order.iter().filter_map(|e| self.entry_track(*e)))
        } else {
            let rest = self
                .cursor
                .map(|c| &self.order[c + 1..])
                .unwrap_or(&self.order[..]);
            Box::new(
                self.user_queue
                    .iter()
                    .chain(rest.iter().filter_map(|i| self.context.get(*i))),
            )
        }
    }

    /// The track that would play after the current one, without
    /// changing state. Mirrors `on_track_end` and `next`, so a caller
    /// can prefetch it. In repeat-one mode, the next track is the
    /// current track.
    pub fn peek_next(&self) -> Option<Track> {
        if self.repeat == RepeatMode::One {
            return self.current().cloned();
        }
        if let Some(entry) = self.manual_order.as_ref().and_then(|o| o.first()) {
            return self.entry_track(*entry).cloned();
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

fn remap_manual_order(
    order: &[UpcomingEntry],
    user_map: &[Option<usize>],
    context_map: &[Option<usize>],
) -> Vec<UpcomingEntry> {
    order
        .iter()
        .filter_map(|entry| match *entry {
            UpcomingEntry::User(index) => user_map
                .get(index)
                .copied()
                .flatten()
                .map(UpcomingEntry::User),
            UpcomingEntry::Context(index) => context_map
                .get(index)
                .copied()
                .flatten()
                .map(UpcomingEntry::Context),
        })
        .collect()
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
            source: Default::default(),
            id: TrackId(id.to_string()),
            title: id.to_string(),
            artists: vec![],
            album: None,
            album_id: None,
            duration: None,
            thumbnail_url: None,
            playlist_item_id: None,
        }
    }

    fn local_track(path: &str) -> Track {
        Track {
            source: super::super::model::MediaSource::LocalFile { path: path.into() },
            id: TrackId(path.into()),
            title: path.into(),
            artists: vec![],
            album: None,
            album_id: None,
            duration: None,
            thumbnail_url: None,
            playlist_item_id: None,
        }
    }

    #[test]
    fn retain_local_remaps_mixed_context_and_queue() {
        let mut queue = Queue::default();
        queue.play_context(
            vec![track("remote"), local_track("one"), local_track("two")],
            2,
            &mut no_random,
        );
        queue.queue_track(track("queued-remote"));
        queue.queue_track(local_track("queued-local"));
        queue.retain_local();
        assert_eq!(queue.context.len(), 2);
        assert_eq!(queue.user_queue.len(), 1);
        assert_eq!(queue.current().unwrap().id, TrackId("two".into()));
        assert!(queue.context.iter().all(Track::is_local));
    }

    #[test]
    fn remove_local_remaps_the_remaining_local_entries() {
        let mut queue = Queue::default();
        queue.play_context(
            vec![local_track("one"), local_track("two")],
            1,
            &mut no_random,
        );
        assert!(queue.remove_local(&TrackId("two".into())));
        assert_eq!(queue.current(), None);
        assert_eq!(queue.context.len(), 1);
        assert_eq!(queue.context[0].id, TrackId("one".into()));
    }

    #[test]
    fn retain_local_remaps_manual_user_indices() {
        let mut queue = Queue::default();
        queue.play_context(vec![local_track("current")], 0, &mut no_random);
        queue.queue_track(track("remote"));
        queue.queue_track(local_track("queued"));
        queue.move_upcoming(&[1], 2);
        queue.retain_local();
        assert_eq!(
            queue
                .upcoming()
                .map(|track| track.id.0.as_str())
                .collect::<Vec<_>>(),
            ["queued"]
        );
        assert_eq!(queue.next().unwrap().id, TrackId("queued".into()));
    }

    #[test]
    fn retain_local_exposes_context_after_removed_current() {
        let mut queue = Queue::default();
        queue.play_context(
            vec![track("remote"), local_track("upcoming")],
            0,
            &mut no_random,
        );
        queue.retain_local();
        assert_eq!(
            queue
                .upcoming()
                .map(|track| track.id.0.as_str())
                .collect::<Vec<_>>(),
            ["upcoming"]
        );
        assert_eq!(queue.next().unwrap().id, TrackId("upcoming".into()));
    }

    #[test]
    fn retain_local_does_not_replay_context_before_removed_current() {
        let mut queue = Queue::default();
        queue.play_context(
            vec![local_track("before"), track("remote"), local_track("after")],
            1,
            &mut no_random,
        );
        queue.retain_local();
        assert_eq!(
            queue
                .upcoming()
                .map(|track| track.id.0.as_str())
                .collect::<Vec<_>>(),
            ["after"]
        );
        assert_eq!(queue.next().unwrap().id, TrackId("after".into()));
    }

    #[test]
    fn repeat_off_does_not_restart_after_exhaustion() {
        let mut queue = Queue::default();
        queue.play_context(vec![track("only")], 0, &mut no_random);
        assert_eq!(queue.next(), None);
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
    fn previous_preserves_pending_manual_edits() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b", "c", "d"]), 0, &mut no_random);
        queue.move_upcoming(&[2], 0);
        queue.previous();
        assert_eq!(
            queue
                .upcoming()
                .map(|t| t.id.0.as_str())
                .collect::<Vec<_>>(),
            ["d", "b", "c"]
        );
        queue.next();
        queue.previous();
        assert_eq!(
            queue
                .upcoming()
                .map(|t| t.id.0.as_str())
                .collect::<Vec<_>>(),
            ["d", "b", "c"]
        );
    }
    #[test]
    fn manual_order_preserves_sources_duplicates_repeat_and_restart() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b", "b", "c"]), 0, &mut no_random);
        queue.queue_track(track("u"));
        queue.queue_track(track("v"));
        queue.move_upcoming(&[2, 4], 0); // b,c,u,v,b
        assert_eq!(
            queue
                .upcoming()
                .map(|t| t.id.0.as_str())
                .collect::<Vec<_>>(),
            ["b", "c", "u", "v", "b"]
        );
        let json = serde_json::to_string(&queue).unwrap();
        queue = serde_json::from_str(&json).unwrap();
        assert_eq!(current_id(&queue), "a");
        assert_eq!(queue.peek_next().unwrap().id.0, "b");
        for id in ["b", "c", "u", "v", "b"] {
            assert_eq!(queue.next().unwrap().id.0, id);
        }
        assert!(queue.next().is_none());
        queue.repeat = RepeatMode::All;
        for id in ["a", "b", "c", "b", "a"] {
            assert_eq!(queue.next().unwrap().id.0, id);
        }
    }
    #[test]
    fn edits_after_reordering_keep_indices_and_current_intact() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b", "c", "d"]), 0, &mut no_random);
        queue.queue_track(track("u"));
        queue.move_upcoming(&[3], 0); // d,u,b,c
        queue.remove_upcoming(2); // b
        queue.queue_track(track("v")); // d,u,v,c
        queue.extend_context(tracks(&["e"]));
        assert_eq!(
            queue
                .upcoming()
                .map(|t| t.id.0.as_str())
                .collect::<Vec<_>>(),
            ["d", "u", "v", "c", "e"]
        );
        queue.clear_user_queue();
        assert_eq!(
            queue
                .upcoming()
                .map(|t| t.id.0.as_str())
                .collect::<Vec<_>>(),
            ["d", "c", "e"]
        );
        let before = queue.clone();
        assert!(queue.jump_to(usize::MAX).is_none());
        assert_eq!(queue, before);
        assert_eq!(queue.jump_to(1).unwrap().id.0, "c");
        assert_eq!(queue.next().unwrap().id.0, "e");
    }
    #[test]
    fn invalid_jump_keeps_explicit_queue() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        queue.queue_track(track("u"));
        let before = queue.clone();
        assert!(queue.jump_to(999).is_none());
        assert_eq!(queue, before);
    }
    #[test]
    fn removing_upcoming_entries_preserves_current_and_repeat_order() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b", "c", "d"]), 0, &mut no_random);
        queue.queue_track(track("u"));
        queue.remove_upcoming(2); // c, after user-queued u and context b.
        assert_eq!(current_id(&queue), "a");
        assert!(!queue.contains(&TrackId("c".into())));
        queue.remove_upcoming(0); // u
        queue.remove_upcoming(usize::MAX); // no effect
        queue.repeat = RepeatMode::All;
        assert_eq!(queue.next().unwrap().id.0, "b");
        assert_eq!(queue.next().unwrap().id.0, "d");
        assert_eq!(queue.next().unwrap().id.0, "a");
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

    #[test]
    fn extend_context_appends_tracks_after_the_context_end() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        queue.next();
        queue.extend_context(tracks(&["c", "d"]));
        let upcoming: Vec<&str> = queue.upcoming().map(|t| t.id.0.as_str()).collect();
        assert_eq!(upcoming, vec!["c", "d"]);
    }

    #[test]
    fn extend_context_lets_next_reach_the_new_tracks() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a"]), 0, &mut no_random);
        assert_eq!(queue.next(), None);
        queue.extend_context(tracks(&["b"]));
        assert_eq!(queue.next().unwrap().id.0, "b");
    }

    #[test]
    fn contains_finds_a_track_already_in_the_context() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        assert!(queue.contains(&TrackId("b".into())));
        assert!(!queue.contains(&TrackId("z".into())));
    }

    #[test]
    fn jump_to_reaches_a_queued_track_and_drops_the_ones_before_it() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        queue.queue_track(track("q1"));
        queue.queue_track(track("q2"));
        assert_eq!(queue.jump_to(1).unwrap().id.0, "q2");
        // q1 was skipped, not kept for a later `next`.
        assert_eq!(queue.next().unwrap().id.0, "b");
    }

    #[test]
    fn jump_to_reaches_a_context_track_and_clears_the_user_queue() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b", "c"]), 0, &mut no_random);
        queue.queue_track(track("q"));
        assert_eq!(queue.jump_to(2).unwrap().id.0, "c");
        assert_eq!(current_id(&queue), "c");
        assert_eq!(queue.upcoming().count(), 0);
    }

    #[test]
    fn jump_to_rejects_an_index_past_the_end() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        assert_eq!(queue.jump_to(5), None);
        assert_eq!(current_id(&queue), "a");
    }

    #[test]
    fn clear_user_queue_drops_only_the_queued_tracks() {
        let mut queue = Queue::default();
        queue.play_context(tracks(&["a", "b"]), 0, &mut no_random);
        queue.queue_track(track("q"));
        queue.clear_user_queue();
        let upcoming: Vec<&str> = queue.upcoming().map(|t| t.id.0.as_str()).collect();
        assert_eq!(upcoming, vec!["b"]);
    }
}
