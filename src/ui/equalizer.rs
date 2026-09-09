//! Shared equalizer controls and preset management.
use crate::core::{
    action::Action,
    equalizer::{LABELS, MAX_DB, builtins},
    state::State,
};

pub fn presets(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    ui.set_max_width(240.);
    let current = state.equalizer.parameters;
    let height = (ui.ctx().content_rect().height() - 160.).clamp(60., 220.);
    egui::ScrollArea::vertical()
        .id_salt("eq-presets-list")
        .max_height(height)
        .show(ui, |ui| {
            for preset in builtins().iter().chain(&state.equalizer.presets) {
                if ui.add(egui::Button::new(&preset.name).truncate()).clicked() {
                    out.push(Action::EqualizerChanged(preset.apply(current)));
                    ui.close();
                }
            }
        });
    ui.separator();
    let id = ui.id().with("eq-preset-name");
    let mut name = ui.data_mut(|data| data.get_temp::<String>(id).unwrap_or_default());
    ui.add(
        egui::TextEdit::singleline(&mut name)
            .hint_text("Preset name")
            .char_limit(40)
            .desired_width(150.),
    );
    ui.data_mut(|data| data.insert_temp(id, name.clone()));
    if ui
        .add_enabled(
            !name.trim().is_empty(),
            egui::Button::new("Save current settings"),
        )
        .clicked()
    {
        out.push(Action::EqualizerPresetSaved(name));
        ui.close();
    }
    if !state.equalizer.presets.is_empty() {
        ui.menu_button("Delete saved preset", |ui| {
            egui::ScrollArea::vertical()
                .id_salt("eq-delete-list")
                .max_height(height)
                .show(ui, |ui| {
                    for preset in &state.equalizer.presets {
                        if ui.add(egui::Button::new(&preset.name).truncate()).clicked() {
                            out.push(Action::EqualizerPresetDeleted(preset.name.clone()));
                            ui.close();
                        }
                    }
                });
        });
    }
}

pub fn controls(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    ui.spacing_mut().slider_width = (ui.available_width() - 190.).clamp(120., 360.);
    let mut p = state.equalizer.parameters;
    ui.horizontal(|ui| {
        let mut value = state.playback.balance;
        let response = super::components::signed_slider(ui, &mut value, 1., "Balance", "");
        if response.double_clicked() || ui.button("Center").clicked() {
            value = 0.;
        }
        response.on_hover_text(balance_label(value));
        if value != state.playback.balance {
            out.push(Action::BalanceSet(value));
        }
    });
    ui.label("L ← Balance → R · independent of EQ");
    ui.separator();
    ui.heading("Equalizer");
    ui.label("Actual range: ±12 dB");
    ui.checkbox(&mut p.enabled, "On");
    ui.checkbox(&mut p.auto_headroom, "Automatic headroom")
        .on_hover_text("Lowers gain by the strongest band boost. Peak protection is always active when EQ is on.");
    super::components::signed_slider(ui, &mut p.preamp_db, MAX_DB, "Preamp", " dB");
    for (gain, label) in p.bands_db.iter_mut().zip(LABELS) {
        super::components::signed_slider(ui, gain, MAX_DB, label, " dB");
    }
    ui.horizontal(|ui| {
        if ui.button("Reset").clicked() {
            p.preamp_db = 0.;
            p.bands_db = [0.; 10];
        }
        ui.menu_button("Presets", |ui| presets(ui, state, out));
    });
    if p != state.equalizer.parameters {
        out.push(Action::EqualizerChanged(p));
    }
}

/// Shared wording for normal and skinned balance controls.
pub fn balance_label(value: f32) -> String {
    if value.abs() < 0.005 {
        "Balance: center · double-click to reset".into()
    } else {
        format!(
            "Balance: {}% {} · double-click to center",
            (value.abs() * 100.).round() as u32,
            if value < 0. { "left" } else { "right" }
        )
    }
}
