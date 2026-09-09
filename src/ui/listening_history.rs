use super::rows::{self, RowContext};
use crate::{
    core::{action::Action, state::State},
    theme::Theme,
};
pub fn view(
    ui: &mut egui::Ui,
    state: &State,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    let history = &state.listening_history;
    ui.horizontal(|ui| {
        super::components::page_title(ui, theme, "YouTube history");
        if ui
            .add_enabled(
                !history.loading,
                super::components::secondary(theme, "Refresh"),
            )
            .clicked()
        {
            out.push(Action::HistoryRequested { more: false });
        }
    });
    ui.weak("Your account’s watch history, including music and other videos. Newest first.");
    if let Some(error) = &history.error {
        ui.horizontal_wrapped(|ui| {
            ui.label(error);
            if ui.button("Retry").clicked() {
                out.push(Action::HistoryRequested {
                    more: history.appending,
                });
            }
        });
    }
    ui.horizontal(|ui| {
        if history.loading {
            ui.spinner();
        }
        if history.continuation.is_some()
            && ui
                .add_enabled(
                    !history.loading,
                    super::components::secondary(theme, "Load older"),
                )
                .clicked()
        {
            out.push(Action::HistoryRequested { more: true });
        }
    });
    if history.loaded && history.tracks.is_empty() {
        ui.label("No playable videos in your YouTube history.");
    }
    rows::track_list(
        ui,
        "youtube-history",
        &history.tracks,
        history.loading,
        theme,
        context,
        out,
    );
}
