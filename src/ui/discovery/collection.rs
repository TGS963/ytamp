use super::{
    home,
    rows::{self, RowContext},
};
use crate::{
    core::{
        action::Action,
        discovery::{Entry, Feed, Target},
        state::{Page, State},
    },
    theme::{TextRole, Theme},
};
use egui::Ui;

use super::{
    shelves::{open, shelf_view},
    status,
};
use crate::ui::components;

pub(crate) fn collection(
    ui: &mut Ui,
    state: &State,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    let Page::Discovery(entry) = &state.page else {
        return;
    };
    let feed = &state.discovery.collection;
    collection_header(ui, entry, feed, theme, out);
    ui.add_space(20.);
    status(ui, feed, out);
    ui.add_space(8.);
    if !feed.page.tracks.is_empty() {
        rows::track_list(
            ui,
            "discovery-tracks",
            &feed.page.tracks,
            feed.loading,
            theme,
            context,
            out,
        );
    } else {
        egui::ScrollArea::vertical()
            .id_salt(("collection-shelves", format!("{:?}", entry.target)))
            .show(ui, |ui| {
                for (i, shelf) in feed.page.shelves.iter().enumerate() {
                    ui.push_id(i, |ui| shelf_view(ui, shelf, theme, context, out));
                }
            });
    }
}

fn collection_header(
    ui: &mut Ui,
    entry: &Entry,
    feed: &Feed,
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        if ui.available_width() > 420. {
            rows::artwork(ui, theme, entry.artwork.as_deref(), 112.);
            ui.add_space(12.);
        }
        ui.vertical(|ui| {
            ui.label(theme.secondary_label(
                TextRole::Caption,
                match &entry.target {
                    Target::Watch { .. } => "YOUR MIX",
                    _ => "EXPLORE",
                },
            ));
            ui.add(egui::Label::new(theme.label(TextRole::Hero, &entry.title).strong()).truncate())
                .on_hover_text(&entry.title);
            ui.add(
                egui::Label::new(theme.secondary_label(TextRole::Body, &entry.subtitle)).truncate(),
            )
            .on_hover_text(&entry.subtitle);
            ui.add_space(10.);
            if !feed.page.tracks.is_empty()
                && ui.add(components::primary(theme, "▶ Play")).clicked()
            {
                out.push(Action::ContextPlayed {
                    tracks: feed.page.tracks.clone(),
                    start: 0,
                });
            }
            if !feed.page.tracks.is_empty() {
                ui.menu_button("More", |ui| {
                    if ui.button("Play next").clicked() {
                        for track in feed.page.tracks.iter().rev() {
                            out.push(Action::TrackPlayNext(track.clone()));
                        }
                        ui.close();
                    }
                    if ui.button("Add to queue").clicked() {
                        for track in &feed.page.tracks {
                            out.push(Action::TrackQueued(track.clone()));
                        }
                        ui.close();
                    }
                });
            }
        });
    });
}

pub(crate) fn shelf_page(
    ui: &mut Ui,
    state: &State,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    let Page::DiscoveryShelf(shelf) = &state.page else {
        return;
    };
    components::page_title(ui, theme, &shelf.title);
    ui.weak(format!("{} items", shelf.entries.len()));
    ui.add_space(16.);
    if shelf.entries.iter().all(|e| e.track.is_some()) {
        let tracks = shelf
            .entries
            .iter()
            .filter_map(|e| e.track.clone())
            .collect::<Vec<_>>();
        rows::track_list(ui, "shelf-tracks", &tracks, false, theme, context, out);
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt(("shelf-all", &shelf.title))
        .show(ui, |ui| {
            let columns = (ui.available_width() / 200.).floor().max(1.) as usize;
            let width = (ui.available_width() - (columns - 1) as f32 * 16.) / columns as f32;
            for chunk in shelf.entries.chunks(columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.;
                    for entry in chunk {
                        let response = home::card(
                            ui,
                            width,
                            &entry.title,
                            &entry.subtitle,
                            entry.artwork.as_deref(),
                            theme,
                        );
                        if response.clicked() {
                            open(entry, out);
                        }
                        if let Some(track) = &entry.track
                            && let Some(action) = rows::row_context_menu(&response, track, context)
                        {
                            out.push(action);
                        }
                    }
                });
                ui.add_space(16.);
            }
        });
}
