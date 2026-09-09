use super::*;

pub(super) fn font(unit: f32) -> f32 {
    (5.0 * unit).clamp(9.0, 14.0)
}

pub(super) fn height_limit(ui: &Ui) -> f32 {
    let frame = ui.spacing().menu_margin.sum().y + 6.0;
    (ui.ctx().content_rect().height() - frame).max(font(1.0))
}

pub(super) fn options_menu(
    popup: egui::Popup<'_>,
    skin: &Skin,
    state: &State,
    unit: f32,
    out: &mut Vec<Action>,
) {
    menu(popup, skin, unit, |ui| {
        crate::ui::local_files::add_button(ui, out);
        options_display_rows(ui, state, unit, out);
        options_skin_rows(ui, state, out);
        options_playback_rows(ui, state, out);
    });
}

pub(super) fn options_display_rows(ui: &mut Ui, state: &State, unit: f32, out: &mut Vec<Action>) {
    ui.set_min_width(font(unit) * 11.0);
    if let Some(action) = scale_menu_row(ui, state.winamp.scale) {
        out.push(action);
    }
    let mut on_top = state.winamp.on_top;
    if ui.checkbox(&mut on_top, "Always on top").clicked() {
        out.push(Action::WinampOnTopToggled);
    }
    ui.separator();
}

pub(super) fn options_skin_rows(ui: &mut Ui, state: &State, out: &mut Vec<Action>) {
    if let Some(action) = skin_menu_rows(ui, &state.winamp.skin, &state.winamp.available_skins) {
        out.push(action);
    }
    if ui.button("Browse skins…").clicked() {
        out.push(Action::SkinBrowserToggled);
        ui.close();
    }
    if ui.button("Open skins folder").clicked() {
        crate::skins_dir::open_folder();
    }
    ui.separator();
}

pub(super) fn options_playback_rows(ui: &mut Ui, state: &State, out: &mut Vec<Action>) {
    playback_error_row(ui, state, out);
    if ui.button("YouTube history").clicked() {
        out.push(Action::NavigatedTo(
            crate::core::state::Page::ListeningHistory,
        ));
        out.push(Action::WinampToggled);
        ui.close();
    }
    if ui.button("Lyrics").clicked() {
        out.push(Action::LyricsToggled);
        ui.close();
    }
    if ui.button("Close Winamp mode").clicked() {
        out.push(Action::WinampToggled);
    }
}

pub(super) fn playback_error_row(ui: &mut Ui, state: &State, out: &mut Vec<Action>) {
    let Some(error) = &state.playback.error else {
        return;
    };
    ui.label(error);
    crate::ui::local_files::recovery(ui, state, out);
    if ui.button("Retry playback").clicked() {
        out.push(Action::PlaybackRetryRequested);
        ui.close();
    }
    ui.separator();
}

/// The 1x through 4x radio row.
pub(super) fn scale_menu_row(ui: &mut Ui, current: u8) -> Option<Action> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.label("Size");
        for candidate in 1..=4u8 {
            let picked = ui
                .selectable_label(candidate == current, format!("{candidate}x"))
                .clicked();
            if picked {
                chosen = Some(Action::WinampScaleSet(candidate));
            }
        }
    });
    chosen
}

/// The built-in skin and every skin in the folder, as a radio list.
pub(super) fn skin_menu_rows(
    ui: &mut Ui,
    current: &Option<String>,
    names: &[String],
) -> Option<Action> {
    let mut chosen = None;
    if ui
        .selectable_label(current.is_none(), "Built-in skin")
        .clicked()
    {
        chosen = Some(Action::SkinChosen(None));
    }
    for name in names {
        let selected = current.as_deref() == Some(name.as_str());
        if ui.selectable_label(selected, name).clicked() {
            chosen = Some(Action::SkinChosen(Some(name.clone())));
        }
    }
    chosen
}

/// A menu styled from the skin's playlist colours, the nearest thing
/// a classic skin says about text on a background. A long list, such
/// as many installed skins, scrolls inside the window rather than
/// running off the screen.
///
/// Ported from fastpotify (MIT, Copyright (c) 2026 Carmine Paolino),
/// src/ui/winamp/mod.rs, `menu` (lines 745-800).
pub(crate) fn menu<R>(
    popup: egui::Popup<'_>,
    skin: &Skin,
    unit: f32,
    contents: impl FnOnce(&mut Ui) -> R,
) -> Option<egui::InnerResponse<R>> {
    let rgb = |[r, g, b]: [u8; 3]| egui::Color32::from_rgb(r, g, b);
    let text = rgb(skin.playlist.normal);
    let current = rgb(skin.playlist.current);
    let background = rgb(skin.playlist.normal_background);
    let selected = rgb(skin.playlist.selected_background);
    let font = font(unit);
    let margin = unit.max(1.0).round();
    let style = move |style: &mut egui::Style| {
        for text_style in [egui::TextStyle::Body, egui::TextStyle::Button] {
            style
                .text_styles
                .insert(text_style, egui::FontId::proportional(font));
        }
        style.spacing.item_spacing = egui::vec2(4.0, 1.0);
        style.spacing.button_padding = egui::vec2(6.0, 1.0);
        // A row is its text and padding, not egui's default 18 points.
        style.spacing.interact_size = egui::vec2(font * 2.0, font + 2.0);
        style.spacing.menu_margin = egui::Margin::same(margin as i8);
        let visuals = &mut style.visuals;
        visuals.window_fill = background;
        visuals.panel_fill = background;
        visuals.window_stroke = egui::Stroke::new(1.0, text.gamma_multiply(0.5));
        visuals.window_corner_radius = egui::CornerRadius::ZERO;
        visuals.menu_corner_radius = egui::CornerRadius::ZERO;
        visuals.window_shadow = egui::Shadow::NONE;
        visuals.popup_shadow = egui::Shadow::NONE;
        visuals.override_text_color = None;
        visuals.selection.bg_fill = selected;
        visuals.selection.stroke = egui::Stroke::new(1.0, current);
        let widgets = &mut visuals.widgets;
        for state in [&mut widgets.noninteractive, &mut widgets.inactive] {
            state.fg_stroke.color = text;
            state.weak_bg_fill = background;
            state.bg_fill = background;
            state.bg_stroke = egui::Stroke::NONE;
        }
        for state in [&mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
            state.fg_stroke.color = current;
            state.weak_bg_fill = selected;
            state.bg_fill = selected;
            state.bg_stroke = egui::Stroke::NONE;
            state.expansion = 0.0;
            state.corner_radius = egui::CornerRadius::ZERO;
        }
    };
    popup.style(style).show(|ui| {
        egui::ScrollArea::vertical()
            .max_height(height_limit(ui))
            .show(ui, contents)
            .inner
    })
}
