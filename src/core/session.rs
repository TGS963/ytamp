//! The saved session: what survives a restart.
//!
//! On startup the last queue comes back with the last track paused at
//! its old position. Play loads the stream and seeks there.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::queue::Queue;
use super::state::State;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedSession {
    pub queue: Queue,
    pub position_secs: u64,
    pub volume: f32,
    /// Missing in a session saved before autoplay existed. Such a
    /// session keeps autoplay on, the default behavior.
    #[serde(default = "default_autoplay")]
    pub autoplay: bool,
}

fn default_autoplay() -> bool {
    true
}

impl SavedSession {
    pub fn capture(state: &State) -> Self {
        Self {
            queue: state.playback.queue.clone(),
            position_secs: state.playback.position.as_secs(),
            volume: state.playback.volume,
            autoplay: state.playback.autoplay,
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
