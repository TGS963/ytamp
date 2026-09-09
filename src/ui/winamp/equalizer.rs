//! Classic skinned equalizer, attached between the player and playlist.
use super::{View, WinampShell};
use crate::core::{
    action::Action,
    equalizer::{LABELS, MAX_DB},
    state::State,
};
use crate::skin::{
    layout::{self, Area},
    sprites,
};
use egui::{Sense, ViewportCommand};

pub(super) fn show(
    view: &mut View,
    state: &State,
    shell: &mut WinampShell,
    out: &mut Vec<Action>,
    focused: bool,
) {
    draw_panel(view, shell.equalizer_shade, focused);
    title_controls(view, shell);
    if shell.equalizer_shade {
        shaded_controls(view, state, out);
        return;
    }
    full_controls(view, state, out);
}

fn draw_panel(view: &mut View, shaded: bool, focused: bool) {
    if shaded {
        view.sprite_at(
            if focused {
                sprites::EQ_SHADE_BAR_ACTIVE
            } else {
                sprites::EQ_SHADE_BAR_INACTIVE
            },
            0,
            0,
        );
    } else {
        view.sprite_at(sprites::EQ_BACKGROUND, 0, 0);
        view.sprite_at(
            if focused {
                sprites::EQ_TITLE_BAR_ACTIVE
            } else {
                sprites::EQ_TITLE_BAR_INACTIVE
            },
            0,
            0,
        );
    }
}

fn title_controls(view: &mut View, shell: &mut WinampShell) {
    let title = view.interact(layout::EQ_TITLE_BAR, "eq-title", Sense::click_and_drag());
    if title.drag_started() {
        view.ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if title.double_clicked() {
        shell.equalizer_shade = !shell.equalizer_shade;
    }
    let pressed = if shell.equalizer_shade {
        sprites::EQ_UNSHADE_BUTTON_PRESSED
    } else {
        sprites::EQ_SHADE_BUTTON_PRESSED
    };
    if view
        .lamp_button(layout::EQ_SHADE, pressed, "eq-shade")
        .on_hover_text("Roll up equalizer")
        .clicked()
    {
        shell.equalizer_shade = !shell.equalizer_shade;
    }
    let (close, close_pressed) = if shell.equalizer_shade {
        (
            sprites::EQ_SHADE_CLOSE_BUTTON,
            sprites::EQ_SHADE_CLOSE_BUTTON_PRESSED,
        )
    } else {
        (sprites::EQ_CLOSE_BUTTON, sprites::EQ_CLOSE_BUTTON_PRESSED)
    };
    if view
        .button(layout::EQ_CLOSE, close, close_pressed, "eq-close")
        .on_hover_text("Close equalizer panel (keeps EQ settings)")
        .clicked()
    {
        shell.equalizer_open = false;
    }
}

fn shaded_controls(view: &mut View, state: &State, out: &mut Vec<Action>) {
    shaded_volume(view, state, out);
    shaded_balance(view, state, out);
}

fn shaded_volume(view: &mut View, state: &State, out: &mut Vec<Action>) {
    let (_, event) = view.slider(
        layout::EQ_SHADE_VOLUME,
        "eq-shade-volume",
        layout::EQ_SHADE_THUMB,
    );
    if let super::SliderEvent::Dragging(value) | super::SliderEvent::Committed(value) = event {
        out.push(Action::VolumeSet(value));
    }
    let x = layout::EQ_SHADE_VOLUME.x
        + (state.playback.volume * (layout::EQ_SHADE_VOLUME.width - layout::EQ_SHADE_THUMB) as f32)
            .round() as u32;
    view.sprite_at(
        shaded_volume_thumb(state.playback.volume),
        x,
        layout::EQ_SHADE_VOLUME.y,
    );
}

fn shaded_volume_thumb(volume: f32) -> crate::skin::Sprite {
    if volume < 0.33 {
        sprites::EQ_SHADE_VOLUME_THUMB_LOW
    } else if volume > 0.66 {
        sprites::EQ_SHADE_VOLUME_THUMB_HIGH
    } else {
        sprites::EQ_SHADE_VOLUME_THUMB_MIDDLE
    }
}

fn shaded_balance(view: &mut View, state: &State, out: &mut Vec<Action>) {
    let (response, event) = view.slider(
        layout::EQ_SHADE_BALANCE,
        "eq-shade-balance",
        layout::EQ_SHADE_THUMB,
    );
    let fraction = shade_balance_fraction(&response, event, state.playback.balance, out);
    let value = fraction * 2. - 1.;
    view.sprite_at(
        shaded_balance_thumb(value),
        layout::EQ_SHADE_BALANCE.x
            + (fraction * (layout::EQ_SHADE_BALANCE.width - layout::EQ_SHADE_THUMB) as f32).round()
                as u32,
        layout::EQ_SHADE_BALANCE.y,
    );
    response.on_hover_text(crate::ui::equalizer::balance_label(value));
}

fn shade_balance_fraction(
    response: &egui::Response,
    event: super::SliderEvent,
    resting: f32,
    out: &mut Vec<Action>,
) -> f32 {
    if response.double_clicked() {
        out.push(Action::BalanceSet(0.));
        return 0.5;
    }
    let fraction = super::slider_fraction(event, (resting + 1.) / 2.);
    if let Some(value) = super::slider_active_value(event) {
        out.push(Action::BalanceSet(value * 2. - 1.));
    }
    fraction
}

fn shaded_balance_thumb(value: f32) -> crate::skin::Sprite {
    if value < -0.33 {
        sprites::EQ_SHADE_BALANCE_THUMB_LEFT
    } else if value > 0.33 {
        sprites::EQ_SHADE_BALANCE_THUMB_RIGHT
    } else {
        sprites::EQ_SHADE_BALANCE_THUMB_MIDDLE
    }
}

fn full_controls(view: &mut View, state: &State, out: &mut Vec<Action>) {
    let mut p = state.equalizer.parameters;
    toggle_buttons(view, &mut p);
    presets_menu(view, state, out);
    let readout = sliders(view, &mut p);
    graph(view, p, sample_rate(state));
    draw_readout(view, readout);
    if p != state.equalizer.parameters {
        out.push(Action::EqualizerChanged(p));
    }
}

fn toggle_buttons(view: &mut View, parameters: &mut crate::core::equalizer::Parameters) {
    if toggle_button(
        view,
        layout::EQ_ON,
        parameters.enabled,
        ToggleSprites {
            on: (sprites::EQ_ON_ON, sprites::EQ_ON_ON_PRESSED),
            off: (sprites::EQ_ON_OFF, sprites::EQ_ON_OFF_PRESSED),
        },
        "eq-enabled",
        "Enable or bypass equalizer",
    ) {
        parameters.enabled = !parameters.enabled;
    }
    if toggle_button(
        view,
        layout::EQ_AUTO,
        parameters.auto_headroom,
        ToggleSprites {
            on: (sprites::EQ_AUTO_ON, sprites::EQ_AUTO_ON_PRESSED),
            off: (sprites::EQ_AUTO_OFF, sprites::EQ_AUTO_OFF_PRESSED),
        },
        "eq-auto",
        "Automatic headroom: reduce gain by the strongest boost. Peak protection stays active.",
    ) {
        parameters.auto_headroom = !parameters.auto_headroom;
    }
}

struct ToggleSprites {
    on: (crate::skin::Sprite, crate::skin::Sprite),
    off: (crate::skin::Sprite, crate::skin::Sprite),
}

fn toggle_button(
    view: &mut View,
    area: Area,
    enabled: bool,
    sprites: ToggleSprites,
    id: &str,
    tooltip: &str,
) -> bool {
    let (normal, pressed) = if enabled { sprites.on } else { sprites.off };
    view.button(area, normal, pressed, id)
        .on_hover_text(tooltip)
        .clicked()
}

fn presets_menu(view: &mut View, state: &State, out: &mut Vec<Action>) {
    let response = view.button(
        layout::EQ_PRESETS_BUTTON,
        sprites::EQ_PRESETS,
        sprites::EQ_PRESETS_PRESSED,
        "eq-presets",
    );
    super::menu(
        egui::Popup::menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
        view.skin,
        view.unit,
        |ui| {
            preset_values(ui, state);
            reset_button(ui, state, out);
            crate::ui::equalizer::presets(ui, state, out);
        },
    );
}

fn preset_values(ui: &mut egui::Ui, state: &State) {
    ui.menu_button("Actual EQ values", |ui| {
        egui::ScrollArea::vertical()
            .max_height(220.)
            .show(ui, |ui| {
                ui.label("Range: ±12 dB");
                ui.label(format!(
                    "Preamp: {:+.1} dB",
                    state.equalizer.parameters.preamp_db
                ));
                for (label, gain) in LABELS.iter().zip(state.equalizer.parameters.bands_db) {
                    ui.label(format!("{label}: {gain:+.1} dB"));
                }
            });
    });
}

fn reset_button(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    if ui.button("Reset to Flat").clicked() {
        out.push(Action::EqualizerChanged(
            crate::core::equalizer::Parameters {
                preamp_db: 0.,
                bands_db: [0.; 10],
                ..state.equalizer.parameters
            },
        ));
        ui.close();
    }
}

fn sliders(view: &mut View, parameters: &mut crate::core::equalizer::Parameters) -> Option<String> {
    let mut readout = slider(
        view,
        layout::EQ_PREAMP,
        "eq-preamp",
        "Preamp",
        &mut parameters.preamp_db,
    );
    for (index, label) in LABELS.iter().enumerate() {
        readout = slider(
            view,
            layout::eq_band(index),
            &format!("eq-band-{index}"),
            label,
            &mut parameters.bands_db[index],
        )
        .or(readout);
    }
    readout
}

fn sample_rate(state: &State) -> u32 {
    if state.playback.sample_rate == 0 {
        44100
    } else {
        state.playback.sample_rate
    }
}

fn draw_readout(view: &mut View, readout: Option<String>) {
    if let Some(readout) = readout {
        view.sprite(sprites::EQ_GRAPH, layout::EQ_GRAPH);
        view.text(&readout, Area::new(88, 19, 109, 6));
        view.text("RANGE +/-12 DB", Area::new(88, 27, 109, 6));
    }
}

fn slider(view: &mut View, area: Area, id: &str, label: &str, gain: &mut f32) -> Option<String> {
    let response = view.interact(area, id, Sense::click_and_drag());
    if response.double_clicked() {
        *gain = 0.;
    } else if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let y = (pos.y - view.origin.y) / view.unit;
        *gain = (MAX_DB - (y - area.y as f32 - 5.5) / 52. * (2. * MAX_DB)).clamp(-MAX_DB, MAX_DB);
    }
    let fraction = (*gain + MAX_DB) / (2. * MAX_DB);
    view.sprite(
        sprites::eq_slider_frame((fraction * 27.).round() as u32),
        area,
    );
    let thumb = if response.is_pointer_button_down_on() {
        sprites::EQ_THUMB_PRESSED
    } else {
        sprites::EQ_THUMB
    };
    view.sprite_at(
        thumb,
        area.x + 1,
        area.y + ((1. - fraction) * 52.).round() as u32,
    );
    let active = response.hovered() || response.dragged();
    response.on_hover_text(format!("{label}: {gain:+.1} dB\nActual range: −12 to +12 dB; skin markings may differ.\nDouble-click to reset"));
    active.then(|| format!("{label} {gain:+.1} DB"))
}

fn graph(view: &View, p: crate::core::equalizer::Parameters, rate: u32) {
    view.sprite(sprites::EQ_GRAPH, layout::EQ_GRAPH);
    let area = layout::EQ_GRAPH;
    let mut previous = None;
    for x in 0..area.width {
        let upper = (rate as f64 * 0.49).clamp(20., 20000.);
        let frequency = 20. * (upper / 20.).powf(x as f64 / (area.width - 1) as f64);
        let db = crate::player::equalizer::response_db(p.bands_db, rate, frequency);
        let y = (9. - db * 9. / 12.).clamp(0., 18.);
        let y = y.round() as u32;
        let previous_y = previous.unwrap_or(y);
        for row in previous_y.min(y)..=previous_y.max(y) {
            let strip = sprites::EQ_GRAPH_LINE;
            if let Some([r, g, b, a]) = view.skin.sheet(strip.sheet).pixel(strip.x, strip.y + row) {
                view.fill(
                    area.x + x,
                    area.y + row,
                    1,
                    1,
                    egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                );
            }
        }
        previous = Some(y);
    }
    let y = (9. - p.effective_preamp_db() * 9. / 12.).clamp(0., 18.) as u32;
    view.sprite_at(sprites::EQ_PREAMP_LINE, area.x, area.y + y);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::update::update;
    struct Harness {
        ctx: egui::Context,
        state: State,
        shell: WinampShell,
        time: f64,
    }
    impl Harness {
        fn new() -> Self {
            let mut h = Self {
                ctx: egui::Context::default(),
                state: State::default(),
                shell: WinampShell::default(),
                time: 0.,
            };
            h.state.winamp.scale = 2;
            h.frame(vec![]);
            h.frame(vec![]);
            h
        }
        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 0.02;
            let mut out = Vec::new();
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 1000.),
                    )),
                    time: Some(self.time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ui, |ui| {
                            super::super::show(ui, &self.state, &mut self.shell, &mut out)
                        });
                },
            );
            output.textures_delta.clear();
            for action in out {
                update(&mut self.state, action, &mut |_| 0);
            }
        }
        fn pointer(&mut self, x: f32, y: f32, down: bool) {
            let pos = egui::pos2(x * 2., y * 2.);
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: down,
                    modifiers: Default::default(),
                },
            ]);
        }
        fn click(&mut self, x: f32, y: f32) {
            self.pointer(x, y, true);
            self.pointer(x, y, false);
            self.frame(vec![]);
        }
    }
    #[test]
    fn skinned_controls_open_enable_adjust_and_close_without_disabling() {
        let mut h = Harness::new();
        h.click(230., 64.);
        assert!(h.shell.equalizer_open);
        assert_eq!(h.shell.stack_height(), 232);
        h.click(27., 140.);
        assert!(h.state.equalizer.parameters.enabled);
        for band in 0..10 {
            h.click(85. + 18. * band as f32, 160.);
            assert!(h.state.equalizer.parameters.bands_db[band] > 10.);
        }
        h.click(28., 212.);
        assert!(h.state.equalizer.parameters.preamp_db < -10.);
        h.click(55., 140.);
        assert!(!h.state.equalizer.parameters.auto_headroom);
        h.click(259., 123.);
        assert!(h.shell.equalizer_shade);
        assert_eq!(h.shell.stack_height(), 130);
        h.click(268., 123.);
        assert!(!h.shell.equalizer_open);
        assert!(h.state.equalizer.parameters.enabled);
        assert_eq!(h.shell.stack_height(), 116);
    }
}
