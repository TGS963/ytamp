//! Equalizer settings shared by both interfaces and the player.
use serde::{Deserialize, Serialize};

pub const FREQUENCIES: [f64; 10] = [
    60., 170., 310., 600., 1000., 3000., 6000., 12000., 14000., 16000.,
];
pub const LABELS: [&str; 10] = [
    "60 Hz", "170 Hz", "310 Hz", "600 Hz", "1 kHz", "3 kHz", "6 kHz", "12 kHz", "14 kHz", "16 kHz",
];
pub const MAX_DB: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Parameters {
    pub enabled: bool,
    pub auto_headroom: bool,
    pub preamp_db: f32,
    pub bands_db: [f32; 10],
}
impl Default for Parameters {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_headroom: true,
            preamp_db: 0.,
            bands_db: [0.; 10],
        }
    }
}
impl Parameters {
    pub fn normalized(mut self) -> Self {
        fn gain(value: f32) -> f32 {
            if value.is_finite() {
                value.clamp(-MAX_DB, MAX_DB)
            } else {
                0.
            }
        }
        self.preamp_db = gain(self.preamp_db);
        self.bands_db = self.bands_db.map(gain);
        self
    }
    pub fn effective_preamp_db(self) -> f32 {
        self.preamp_db
            - if self.auto_headroom {
                self.bands_db.into_iter().fold(0., f32::max)
            } else {
                0.
            }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub preamp_db: f32,
    pub bands_db: [f32; 10],
}
impl Preset {
    pub fn apply(&self, current: Parameters) -> Parameters {
        Parameters {
            preamp_db: self.preamp_db,
            bands_db: self.bands_db,
            ..current
        }
        .normalized()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqualizerSettings {
    pub parameters: Parameters,
    pub presets: Vec<Preset>,
}
impl EqualizerSettings {
    pub fn normalized(mut self) -> Self {
        self.parameters = self.parameters.normalized();
        self.presets.truncate(32);
        for preset in &mut self.presets {
            preset.name = preset.name.trim().chars().take(40).collect();
            let p = preset.apply(Parameters::default());
            preset.preamp_db = p.preamp_db;
            preset.bands_db = p.bands_db;
        }
        self.presets.retain(|p| !p.name.is_empty());
        self
    }
    pub fn save_preset(&mut self, name: &str) -> bool {
        let name: String = name.trim().chars().take(40).collect();
        if name.is_empty() {
            return false;
        }
        let preset = Preset {
            name,
            preamp_db: self.parameters.preamp_db,
            bands_db: self.parameters.bands_db,
        };
        if let Some(existing) = self.presets.iter_mut().find(|p| p.name == preset.name) {
            *existing = preset;
        } else if self.presets.len() < 32 {
            self.presets.push(preset);
        } else {
            return false;
        }
        true
    }
}

pub fn builtins() -> [Preset; 4] {
    [
        ("Flat", [0.; 10]),
        ("Warm", [3., 2., 1., 0., 0., -1., -1., 0., 0., 0.]),
        ("Vocal", [-2., -1., 0., 1., 2., 3., 1., 0., -1., -1.]),
        ("Bright", [-1., -1., 0., 0., 0., 1., 2., 3., 2., 1.]),
    ]
    .map(|(name, bands_db)| Preset {
        name: name.into(),
        preamp_db: 0.,
        bands_db,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        action::Action,
        effect::{Effect, PlayerCommand},
        session::SavedSession,
        state::State,
        update::update,
    };
    fn apply(state: &mut State, action: Action) -> Vec<Effect> {
        update(state, action, &mut |_| 0)
    }
    #[test]
    fn settings_and_presets_restore_and_reach_the_player() {
        let mut state = State::default();
        let p = Parameters {
            enabled: true,
            preamp_db: -3.,
            bands_db: [2.; 10],
            ..Parameters::default()
        };
        assert_eq!(
            apply(&mut state, Action::EqualizerChanged(p)),
            vec![Effect::Player(PlayerCommand::SetEqualizer(p))]
        );
        apply(&mut state, Action::EqualizerPresetSaved(" Mine ".into()));
        let saved =
            SavedSession::from_json(&SavedSession::capture(&state).to_json().unwrap()).unwrap();
        let mut restored = State::default();
        let effects = apply(&mut restored, Action::SessionRestored(Box::new(saved)));
        assert_eq!(restored.equalizer, state.equalizer);
        assert!(effects.contains(&Effect::Player(PlayerCommand::SetEqualizer(p))));
        apply(&mut restored, Action::SignOutRequested);
        assert_eq!(restored.equalizer, state.equalizer);
        apply(&mut restored, Action::EqualizerPresetDeleted("Mine".into()));
        assert!(restored.equalizer.presets.is_empty());
    }
    #[test]
    fn old_sessions_default_to_bypass_and_invalid_gains_are_sanitized() {
        let saved = SavedSession::capture(&State::default());
        let mut json = serde_json::to_value(saved).unwrap();
        json.as_object_mut().unwrap().remove("equalizer");
        assert_eq!(
            SavedSession::from_json(&json.to_string())
                .unwrap()
                .equalizer,
            EqualizerSettings::default()
        );
        let mut state = State::default();
        apply(
            &mut state,
            Action::EqualizerChanged(Parameters {
                enabled: true,
                preamp_db: f32::NAN,
                bands_db: [100.; 10],
                ..Parameters::default()
            }),
        );
        assert_eq!(state.equalizer.parameters.preamp_db, 0.);
        assert_eq!(state.equalizer.parameters.bands_db, [12.; 10]);
        assert!(SavedSession::capture(&state).to_json().is_some());
    }
    #[test]
    fn named_presets_replace_without_duplicates_and_are_bounded() {
        let mut settings = EqualizerSettings::default();
        assert!(!settings.save_preset("  "));
        assert!(settings.save_preset("Custom"));
        settings.parameters.preamp_db = -4.;
        assert!(settings.save_preset("Custom"));
        assert_eq!(settings.presets.len(), 1);
        assert_eq!(settings.presets[0].preamp_db, -4.);
        for i in 0..31 {
            assert!(settings.save_preset(&i.to_string()));
        }
        assert!(!settings.save_preset("Overflow"));
        assert!(settings.save_preset("Custom"));
    }
}
