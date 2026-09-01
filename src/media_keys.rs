//! OS media integration: media keys in, now-playing metadata out.
//!
//! souvlaki backs this with MPRIS on Linux, the system transport
//! controls on Windows, and MPNowPlayingInfoCenter on macOS.

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};

use crate::core::action::Action;
use crate::core::model::TrackId;
use crate::core::state::{PlayStatus, State};

pub struct MediaKeys {
    controls: Option<MediaControls>,
    shown_track: Option<TrackId>,
    shown_status: PlayStatus,
}

impl MediaKeys {
    /// Attaches to the OS controls. A failure downgrades to a no-op
    /// integration and a log line, never a broken app.
    pub fn attach(deliver: impl Fn(Action) + Send + 'static) -> Self {
        let config = PlatformConfig {
            display_name: "ytamp",
            dbus_name: "ytamp",
            hwnd: None,
        };
        let controls = MediaControls::new(config)
            .and_then(|mut controls| {
                controls.attach(move |event| {
                    if let Some(action) = event_action(event) {
                        deliver(action);
                    }
                })?;
                Ok(controls)
            })
            .inspect_err(|error| log::warn!("media keys unavailable: {error:?}"))
            .ok();
        Self {
            controls,
            shown_track: None,
            shown_status: PlayStatus::Stopped,
        }
    }

    /// Mirrors the state into the OS controls. Cheap when nothing changed.
    pub fn sync(&mut self, state: &State) {
        let Some(controls) = &mut self.controls else {
            return;
        };
        let track_id = state.playback.queue.current().map(|track| track.id.clone());
        if track_id != self.shown_track {
            self.shown_track = track_id;
            show_metadata(controls, state);
        }
        if state.playback.status != self.shown_status {
            self.shown_status = state.playback.status;
            show_playback(controls, state);
        }
    }
}

fn event_action(event: MediaControlEvent) -> Option<Action> {
    match event {
        MediaControlEvent::Play | MediaControlEvent::Pause | MediaControlEvent::Toggle => {
            Some(Action::PlayToggled)
        }
        MediaControlEvent::Next => Some(Action::NextPressed),
        MediaControlEvent::Previous => Some(Action::PreviousPressed),
        MediaControlEvent::SetPosition(MediaPosition(position)) => {
            Some(Action::SeekRequested(position))
        }
        _ => None,
    }
}

fn show_metadata(controls: &mut MediaControls, state: &State) {
    let Some(track) = state.playback.queue.current() else {
        let _ = controls.set_playback(MediaPlayback::Stopped);
        return;
    };
    let artist = track.artists.join(", ");
    let metadata = MediaMetadata {
        title: Some(&track.title),
        artist: Some(&artist),
        album: track.album.as_deref(),
        cover_url: track.thumbnail_url.as_deref(),
        duration: track.duration,
    };
    if let Err(error) = controls.set_metadata(metadata) {
        log::warn!("now-playing metadata failed: {error:?}");
    }
}

fn show_playback(controls: &mut MediaControls, state: &State) {
    let progress = Some(MediaPosition(state.playback.position));
    let playback = match state.playback.status {
        PlayStatus::Playing | PlayStatus::Loading => MediaPlayback::Playing { progress },
        PlayStatus::Paused => MediaPlayback::Paused { progress },
        PlayStatus::Stopped => MediaPlayback::Stopped,
    };
    if let Err(error) = controls.set_playback(playback) {
        log::warn!("playback status update failed: {error:?}");
    }
}
