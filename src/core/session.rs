//! The saved session: what survives a restart.
//!
//! On startup the last queue comes back with the last track paused at
//! its old position. Play loads the stream and seeks there.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::queue::Queue;
use super::state::{State, WinampSettings};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSession {
    #[serde(default)]
    pub lyrics_delays: std::collections::BTreeMap<String, f64>,
    #[serde(default)]
    pub equalizer: super::equalizer::EqualizerSettings,
    pub queue: Queue,
    pub position_secs: u64,
    #[serde(default)]
    pub track_duration_secs: Option<u64>,
    pub volume: f32,
    #[serde(default)]
    pub balance: f32,
    /// Missing in a session saved before autoplay existed. Such a
    /// session keeps autoplay on, the default behavior.
    #[serde(default = "default_autoplay")]
    pub autoplay: bool,
    /// Missing in a session saved before the Winamp window existed.
    /// Such a session starts with the window closed, the default.
    #[serde(default)]
    pub winamp: WinampSettings,
}

fn default_autoplay() -> bool {
    true
}

impl SavedSession {
    pub fn capture(state: &State) -> Self {
        Self {
            lyrics_delays: state.lyrics.delays.clone(),
            equalizer: state.equalizer.clone(),
            queue: state.playback.queue.clone(),
            position_secs: state.playback.position.as_secs(),
            track_duration_secs: state.playback.track_duration.map(|d| d.as_secs()),
            volume: state.playback.volume,
            balance: state.playback.balance,
            autoplay: state.playback.autoplay,
            winamp: state.winamp.clone(),
        }
    }

    pub fn position(&self) -> Duration {
        Duration::from_secs(self.position_secs)
    }

    pub fn to_json(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_duration_survives_restart_and_old_sessions_remain_readable() {
        let mut state = State::default();
        state.playback.track_duration = Some(Duration::from_secs(201));
        let saved = SavedSession::capture(&state);
        let json = saved.to_json().unwrap();
        assert_eq!(
            SavedSession::from_json(&json).unwrap().track_duration_secs,
            Some(201)
        );
        let mut old = serde_json::to_value(saved).unwrap();
        old.as_object_mut().unwrap().remove("track_duration_secs");
        assert_eq!(
            SavedSession::from_json(&old.to_string())
                .unwrap()
                .track_duration_secs,
            None
        );
    }
    #[test]
    fn lyric_delays_survive_restart_and_reset_with_old_session_compatibility() {
        use crate::core::{action::Action, model::TrackId, update::update};
        let mut state = State::default();
        update(
            &mut state,
            Action::LyricsDelaySet {
                track: TrackId("song".into()),
                seconds: 1.3,
            },
            &mut |_| 0,
        );
        let saved =
            SavedSession::from_json(&SavedSession::capture(&state).to_json().unwrap()).unwrap();
        let mut restored = State::default();
        update(
            &mut restored,
            Action::SessionRestored(Box::new(saved.clone())),
            &mut |_| 0,
        );
        assert_eq!(restored.lyrics.delays.get("song"), Some(&1.3));
        update(
            &mut restored,
            Action::LyricsDelaySet {
                track: TrackId("song".into()),
                seconds: 0.0,
            },
            &mut |_| 0,
        );
        assert!(restored.lyrics.delays.is_empty());
        let mut old = serde_json::to_value(saved).unwrap();
        old.as_object_mut().unwrap().remove("lyrics_delays");
        assert!(
            SavedSession::from_json(&old.to_string())
                .unwrap()
                .lyrics_delays
                .is_empty()
        );
    }
    #[test]
    fn balance_restores_reaches_player_and_old_sessions_default_to_center() {
        use crate::core::{
            action::Action,
            effect::{Effect, PlayerCommand},
            update::update,
        };
        let mut state = State::default();
        let effects = update(&mut state, Action::BalanceSet(-0.75), &mut |_| 0);
        assert!(effects.contains(&Effect::Player(PlayerCommand::SetBalance(-0.75))));
        let saved =
            SavedSession::from_json(&SavedSession::capture(&state).to_json().unwrap()).unwrap();
        let mut restored = State::default();
        let effects = update(
            &mut restored,
            Action::SessionRestored(Box::new(saved.clone())),
            &mut |_| 0,
        );
        assert_eq!(restored.playback.balance, -0.75);
        assert!(effects.contains(&Effect::Player(PlayerCommand::SetBalance(-0.75))));
        update(&mut restored, Action::SignOutRequested, &mut |_| 0);
        assert_eq!(restored.playback.balance, -0.75);
        let mut json = serde_json::to_value(saved).unwrap();
        json.as_object_mut().unwrap().remove("balance");
        assert_eq!(
            SavedSession::from_json(&json.to_string()).unwrap().balance,
            0.
        );
        update(&mut restored, Action::BalanceSet(f32::NAN), &mut |_| 0);
        assert_eq!(restored.playback.balance, 0.);
        update(&mut restored, Action::BalanceSet(4.), &mut |_| 0);
        assert_eq!(restored.playback.balance, 1.);
    }
    #[test]
    fn a_session_survives_the_json_round_trip() {
        let mut state = State::default();
        state.playback.position = Duration::from_secs(42);
        state.playback.volume = 0.5;
        let saved = SavedSession::capture(&state);
        let restored = SavedSession::from_json(&saved.to_json().expect("json")).expect("parse");
        assert_eq!(restored.position(), Duration::from_secs(42));
        assert_eq!(restored.volume, 0.5);
    }
}
