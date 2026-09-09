//! File actions shared by the default and classic views.
use crate::core::{action::Action, state::State};
use std::path::PathBuf;

pub fn dropped(ctx: &egui::Context) -> Vec<Action> {
    let paths = ctx.input_mut(|input| {
        std::mem::take(&mut input.raw.dropped_files)
            .into_iter()
            .map(|file| file.path().to_path_buf())
            .collect::<Vec<_>>()
    });
    route(paths)
}
fn route(paths: Vec<PathBuf>) -> Vec<Action> {
    let (skins, media): (Vec<_>, Vec<_>) = paths
        .into_iter()
        .partition(|path| super::winamp::is_skin_file(path));
    let mut actions: Vec<_> = skins.into_iter().map(Action::SkinFileDropped).collect();
    if !media.is_empty() {
        actions.push(Action::LocalFilesDropped(media));
    }
    actions
}
pub fn drop_hint(ctx: &egui::Context) {
    if !ctx.input(|input| !input.raw.hovered_files.is_empty()) {
        return;
    }
    let rect = ctx.content_rect().shrink(4.);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("file-drop-hint"),
    ));
    painter.rect_filled(rect, 4., egui::Color32::from_black_alpha(180));
    painter.rect_stroke(
        rect,
        4.,
        egui::Stroke::new(2., egui::Color32::from_rgb(255, 65, 96)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "Drop files to add to queue",
        egui::FontId::proportional(16.),
        egui::Color32::WHITE,
    );
}
pub fn shortcut(ctx: &egui::Context, out: &mut Vec<Action>) {
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) {
        out.push(Action::AddFilesRequested);
    }
}
pub fn add_button(ui: &mut egui::Ui, out: &mut Vec<Action>) {
    if ui.button("Add files…").clicked() {
        out.push(Action::AddFilesRequested);
        close_menu_or_popup(ui);
    }
}
pub fn progress(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    if !state.imports.pending() {
        return;
    }
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(format!(
            "Reading files: {} / {}",
            state.imports.done,
            state.imports.total()
        ));
        if ui.small_button("Cancel").clicked() {
            out.push(Action::LocalImportsCancelled);
        }
    });
}
pub fn recovery(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    let Some(track) = state
        .playback
        .queue
        .current()
        .filter(|track| track.is_local())
    else {
        return;
    };
    if ui.button("Locate file…").clicked() {
        out.push(Action::LocalFileLocateRequested(track.id.clone()));
        close_menu_or_popup(ui);
    }
    if ui.button("Remove file from queue").clicked() {
        out.push(Action::LocalFileRemoveRequested(track.id.clone()));
        close_menu_or_popup(ui);
    }
}

fn close_menu_or_popup(ui: &egui::Ui) {
    if ui
        .stack()
        .iter()
        .any(|stack| matches!(stack.kind(), Some(egui::UiKind::Menu | egui::UiKind::Popup)))
    {
        ui.close();
    }
}
pub fn classic_status(ctx: &egui::Context, state: &State, out: &mut Vec<Action>) {
    if !state.imports.pending() && state.notices.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("local-import-status"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::LEFT_BOTTOM, [4., -4.])
        .show(ctx, |ui| {
            ui.style_mut().override_font_id = Some(egui::FontId::proportional(11.));
            egui::Frame::popup(ui.style())
                .inner_margin(5.)
                .show(ui, |ui| {
                    ui.set_max_width((ctx.content_rect().width() - 20.).max(120.));
                    if state.imports.pending() {
                        progress(ui, state, out);
                    } else {
                        classic_result(ui, state, out);
                    }
                });
        });
}
fn classic_result(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    let Some(notice) = state.notices.last() else {
        return;
    };
    ui.add(egui::Label::new(notice.lines().next().unwrap_or(notice)).wrap());
    ui.horizontal(|ui| {
        if ui.small_button("Details").clicked() {
            out.push(Action::WinampToggled);
        }
        if ui.small_button("Dismiss").clicked() {
            out.extend((0..state.notices.len()).rev().map(Action::NoticeDismissed));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_drop_keeps_media_order_and_routes_skins_separately() {
        let actions = route(vec![
            "first.mp3".into(),
            "skin.WSZ".into(),
            "second.mp4".into(),
            "skin.zip".into(),
        ]);
        assert!(
            matches!(&actions[0], Action::SkinFileDropped(p) if p == &PathBuf::from("skin.WSZ"))
        );
        assert!(
            matches!(&actions[2], Action::LocalFilesDropped(paths) if paths == &[PathBuf::from("first.mp3"), PathBuf::from("second.mp4")])
        );
    }
}
