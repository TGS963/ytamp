use super::{Action, ApiRequest, Effect, RandomBelow, State, play_context};

pub(super) fn apply(state: &mut State, action: Action, random_below: RandomBelow) -> Vec<Effect> {
    match action {
        Action::RadioStartCancelled => {
            state.discovery.radio_loading = false;
            state.discovery.radio_request_id = state.discovery.radio_request_id.wrapping_add(1);
            vec![]
        }
        Action::RadioStartRequested(seed) => {
            if seed.is_local() {
                return vec![];
            }
            state.discovery.radio_loading = true;
            state.discovery.radio_request_id = state.discovery.radio_request_id.wrapping_add(1);
            vec![Effect::Api(ApiRequest::StartRadio {
                request_id: state.discovery.radio_request_id,
                playback_generation: state.playback_generation,
                seed,
            })]
        }
        Action::RadioStarted {
            request_id,
            playback_generation,
            seed,
            result,
        } => {
            if seed.is_local() {
                state.discovery.radio_loading = false;
                return vec![];
            }
            if request_id != state.discovery.radio_request_id {
                return vec![];
            }
            state.discovery.radio_loading = false;
            if playback_generation != state.playback_generation {
                return vec![];
            }
            state.discovery.radio_request_id = state.discovery.radio_request_id.wrapping_add(1);
            match result {
                Ok(mut tracks) if !tracks.is_empty() => {
                    tracks.retain(|t| t.id != seed.id);
                    tracks.insert(0, seed);
                    play_context(state, tracks, 0, random_below)
                }
                Ok(_) => {
                    state
                        .notices
                        .push("No radio is available for this song.".into());
                    vec![]
                }
                Err(e) => {
                    state.notices.push(format!("Couldn’t start radio: {e}"));
                    vec![]
                }
            }
        }
        _ => unreachable!("action routed to the wrong reducer domain"),
    }
}
