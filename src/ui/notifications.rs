use crate::core::{action::Action, state::State};
use crate::theme::{ColorRole, TextRole, Theme};

use super::local_files;

use super::player_bar::HEIGHT as PLAYER_HEIGHT;
const CARD_MAX_WIDTH: f32 = 420.0;
const CARD_MARGIN: f32 = 12.0;
const DETAILS_MAX_HEIGHT: f32 = 150.0;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PlaybackCardState {
    identity: String,
    collapsed: bool,
}

pub(super) fn view(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    if state.playback.error.is_none() {
        clear_playback_card_state(ui.ctx());
    }
    let has_activity = state.playback.error.is_some()
        || !state.notices.is_empty()
        || state.imports.pending()
        || state.discovery.radio_loading;
    if !has_activity {
        return;
    }

    let content = ui.ctx().content_rect();
    let width = (content.width() - CARD_MARGIN * 2.0).min(CARD_MAX_WIDTH);
    let max_height = (content.height() - PLAYER_HEIGHT - CARD_MARGIN * 2.0).max(100.0);
    egui::Area::new(egui::Id::new("notifications"))
        .order(egui::Order::Foreground)
        .anchor(
            egui::Align2::RIGHT_BOTTOM,
            [-CARD_MARGIN, -(PLAYER_HEIGHT + CARD_MARGIN)],
        )
        .show(ui.ctx(), |ui| {
            ui.set_width(width);
            ui.set_max_height(max_height);
            let frame = egui::Frame::new()
                .fill(theme.color(ColorRole::Surface))
                .stroke(egui::Stroke::new(1.0, theme.color(ColorRole::Border)))
                .corner_radius(egui::CornerRadius::same(
                    theme
                        .metric(crate::theme::MetricRole::CornerRadius)
                        .clamp(0.0, 255.0) as u8,
                ))
                .inner_margin(egui::Margin::same(12));
            frame.show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height((max_height - 24.0).max(60.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| draw_card(ui, state, theme, out));
            });
        });
}

fn draw_card(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    if state.imports.pending() {
        local_files::progress(ui, state, out);
    }
    if state.discovery.radio_loading {
        radio_progress(ui, out);
    }
    if state.playback.error.is_some() {
        playback_card(ui, state, theme, out);
    }
    if !state.notices.is_empty() {
        notice_card(ui, state, theme, out);
    }
}

fn radio_progress(ui: &mut egui::Ui, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label("Starting radio…");
        if ui.small_button("Cancel").clicked() {
            out.push(Action::RadioStartCancelled);
        }
    });
}

fn playback_card(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let Some(error) = state.playback.error.as_deref() else {
        return;
    };
    let track = state.playback.queue.current();
    let identity = format!(
        "{}:{}",
        track.map(|item| item.id.0.as_str()).unwrap_or(""),
        error
    );
    let collapsed = playback_is_collapsed(ui.ctx(), &identity);
    if collapsed {
        if ui.button("Playback stopped").clicked() {
            set_playback_collapsed(ui.ctx(), identity, false);
        }
        return;
    }

    ui.horizontal(|ui| {
        ui.label(theme.label(TextRole::Heading, "Playback stopped").strong());
        if ui.small_button("Minimize").clicked() {
            set_playback_collapsed(ui.ctx(), identity.clone(), true);
        }
    });
    if let Some(track) = track {
        ui.add(egui::Label::new(theme.label(TextRole::Body, &track.title)).truncate());
    }
    ui.horizontal_wrapped(|ui| {
        if ui
            .add(
                egui::Button::new(
                    theme
                        .label(TextRole::Body, "Retry")
                        .color(theme.color(ColorRole::OnAccent)),
                )
                .fill(theme.color(ColorRole::Accent)),
            )
            .clicked()
        {
            out.push(Action::PlaybackRetryRequested);
        }
        local_files::recovery(ui, state, out);
    });
    egui::CollapsingHeader::new("Technical details")
        .id_salt("playback-technical-details")
        .show(ui, |ui| {
            details_text(ui, error, DETAILS_MAX_HEIGHT);
        });
}

fn notice_card(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let index = state.notices.len() - 1;
    let notice = &state.notices[index];
    if state.playback.error.is_some() {
        ui.separator();
    }
    ui.horizontal(|ui| {
        ui.label(theme.label(TextRole::Heading, "Notice"));
        if state.notices.len() > 1 {
            ui.weak(format!("{} notices", state.notices.len()));
        }
        if ui.small_button("Dismiss").clicked() {
            out.push(Action::NoticeDismissed(index));
        }
    });
    ui.label(notice_preview(notice));
    egui::CollapsingHeader::new("Details")
        .id_salt("notice-details")
        .show(ui, |ui| details_text(ui, notice, DETAILS_MAX_HEIGHT));
}

fn details_text(ui: &mut egui::Ui, text: &str, max_height: f32) {
    egui::ScrollArea::vertical()
        .max_height(max_height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.add(egui::Label::new(text).selectable(true).wrap());
        });
    if ui.small_button("Copy details").clicked() {
        ui.ctx().copy_text(text.to_owned());
    }
}

fn notice_preview(notice: &str) -> String {
    let line = notice.lines().next().unwrap_or(notice);
    let mut preview: String = line.chars().take(140).collect();
    if line.chars().count() > 140 {
        preview.push('…');
    }
    preview
}

fn playback_is_collapsed(ctx: &egui::Context, identity: &str) -> bool {
    let id = egui::Id::new("playback-card-state");
    ctx.data_mut(|data| match data.get_temp::<PlaybackCardState>(id) {
        Some(saved) if saved.identity == identity => saved.collapsed,
        _ => {
            data.insert_temp(
                id,
                PlaybackCardState {
                    identity: identity.to_owned(),
                    collapsed: false,
                },
            );
            false
        }
    })
}

fn set_playback_collapsed(ctx: &egui::Context, identity: String, collapsed: bool) {
    ctx.data_mut(|data| {
        data.insert_temp(
            egui::Id::new("playback-card-state"),
            PlaybackCardState {
                identity,
                collapsed,
            },
        );
    });
}

fn clear_playback_card_state(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.remove::<PlaybackCardState>(egui::Id::new("playback-card-state"));
    });
}
