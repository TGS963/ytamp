use super::{Action, Effect, PlayerCommand, State, set_volume};

pub(super) fn apply(state: &mut State, action: Action) -> Vec<Effect> {
    match action {
        Action::VolumeSet(volume) => set_volume(state, volume),
        Action::BalanceSet(value) => {
            let value = if value.is_finite() {
                value.clamp(-1., 1.)
            } else {
                0.
            };
            state.playback.balance = value;
            vec![Effect::Player(PlayerCommand::SetBalance(value))]
        }
        Action::EqualizerChanged(parameters) => {
            let parameters = parameters.normalized();
            if state.equalizer.parameters == parameters {
                return vec![];
            }
            state.equalizer.parameters = parameters;
            vec![Effect::Player(PlayerCommand::SetEqualizer(parameters))]
        }
        Action::EqualizerPresetSaved(name) => {
            if !state.equalizer.save_preset(&name) {
                state
                    .notices
                    .push("Enter a preset name; at most 32 custom presets can be saved.".into());
            }
            vec![]
        }
        Action::EqualizerPresetDeleted(name) => {
            state.equalizer.presets.retain(|p| p.name != name);
            vec![]
        }
        _ => unreachable!("action routed to the wrong reducer domain"),
    }
}
