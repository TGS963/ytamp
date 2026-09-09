use super::{
    home,
    rows::{self, RowContext},
};
use crate::{
    core::{action::Action, discovery::Feed, state::State},
    theme::{TextRole, Theme},
};
use egui::{Ui, vec2};

mod collection;
mod shelves;
pub(crate) use collection::{collection, shelf_page};
pub(crate) use shelves::shelf_view;
#[cfg(test)]
mod tests;
pub fn feed(
    ui: &mut Ui,
    state: &State,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    let feed = &state.discovery.home;
    if super::components::section(
        ui,
        theme,
        "Discover your next favorite",
        if feed.loading && feed.loaded {
            "Refreshing…"
        } else {
            "Refresh"
        },
        !feed.loading,
    )
    .clicked()
    {
        out.push(Action::DiscoveryRequested { more: false });
    }
    ui.label(theme.secondary_label(
        TextRole::Caption,
        "Music, mixes, and discoveries from YouTube",
    ));
    ui.add_space(20.);
    for (i, shelf) in feed.page.shelves.iter().enumerate() {
        ui.push_id(("discovery", i), |ui| {
            shelf_view(ui, shelf, theme, context, out)
        });
    }
    status(ui, feed, out);
    ui.add_space(28.);
}
pub(super) fn status(ui: &mut Ui, feed: &Feed, out: &mut Vec<Action>) {
    render_loading_status(ui, feed);
    render_error_status(ui, feed, out);
    render_empty_status(ui, feed);
    render_more_button(ui, feed, out);
}

fn render_loading_status(ui: &mut Ui, feed: &Feed) {
    if !feed.loading || (feed.loaded && !feed.appending) {
        return;
    }
    ui.horizontal(|ui| {
        ui.spinner();
        ui.weak(if feed.loaded {
            "Finding more music…"
        } else {
            "Finding your next favorite…"
        });
    });
}

fn render_error_status(ui: &mut Ui, feed: &Feed, out: &mut Vec<Action>) {
    let Some(error) = &feed.error else { return };
    ui.horizontal_wrapped(|ui| {
        ui.weak(error);
        if ui.button("Try again").clicked() {
            out.push(Action::DiscoveryRequested {
                more: feed.appending,
            });
        }
    });
}

fn render_empty_status(ui: &mut Ui, feed: &Feed) {
    if feed.loaded && feed.page.shelves.is_empty() && feed.page.tracks.is_empty() {
        ui.weak("Nothing here just yet. Your music is still waiting in Library.");
    }
}

fn render_more_button(ui: &mut Ui, feed: &Feed, out: &mut Vec<Action>) {
    if feed.page.continuation.is_none() {
        return;
    }
    if ui
        .add_enabled(
            !feed.loading,
            egui::Button::new("Explore more ↓").min_size(vec2(150., 36.)),
        )
        .clicked()
    {
        out.push(Action::DiscoveryRequested { more: true });
    }
}
