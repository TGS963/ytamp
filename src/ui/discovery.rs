use super::{
    home,
    rows::{self, RowContext},
};
use crate::{
    core::{
        action::Action,
        discovery::{Entry, Feed, Shelf, Target},
        state::{Page, State},
    },
    theme::{ColorRole, TextRole, Theme},
};
use egui::{Ui, vec2};
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
fn status(ui: &mut Ui, feed: &Feed, out: &mut Vec<Action>) {
    if feed.loading && (!feed.loaded || feed.appending) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.weak(if feed.loaded {
                "Finding more music…"
            } else {
                "Finding your next favorite…"
            });
        });
    }
    if let Some(error) = &feed.error {
        ui.horizontal_wrapped(|ui| {
            ui.weak(error);
            if ui.button("Try again").clicked() {
                out.push(Action::DiscoveryRequested {
                    more: feed.appending,
                });
            }
        });
    }
    if feed.loaded && feed.page.shelves.is_empty() && feed.page.tracks.is_empty() {
        ui.weak("Nothing here just yet. Your music is still waiting in Library.");
    }
    if feed.page.continuation.is_some()
        && ui
            .add_enabled(
                !feed.loading,
                egui::Button::new("Explore more ↓").min_size(vec2(150., 36.)),
            )
            .clicked()
    {
        out.push(Action::DiscoveryRequested { more: true });
    }
}
fn shelf_view(
    ui: &mut Ui,
    shelf: &Shelf,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    let all_tracks = shelf.entries.iter().all(|e| e.track.is_some());
    let available = ui.available_width();
    let card_columns =
        (((available + 16.) / 190.).floor().max(1.) as usize).min(shelf.entries.len().max(1));
    let card_width = (available - (card_columns - 1) as f32 * 16.) / card_columns as f32;
    let max_offset = ((card_width + 16.) * shelf.entries.len() as f32 - 16. - available).max(0.);
    let scroll_id = ui.id().with("shelf-offset");
    let mut offset = ui
        .data(|data| data.get_temp::<f32>(scroll_id))
        .unwrap_or(0.);
    ui.horizontal(|ui| {
        let title_width = (ui.available_width() - 170.).max(80.);
        ui.allocate_ui_with_layout(
            vec2(title_width, 32.),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(title_width);
                super::components::heading(ui, theme, &shelf.title);
            },
        );
        if ui.add(super::components::quiet("See all")).clicked() {
            out.push(Action::DiscoveryShelfOpened(shelf.clone()));
        }
        if !all_tracks && shelf.entries.len() > card_columns {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(offset < max_offset - 1., egui::Button::new("›").small())
                    .on_hover_text("More in this shelf")
                    .clicked()
                {
                    offset += (card_width + 16.) * card_columns as f32;
                }
                if ui
                    .add_enabled(offset > 0., egui::Button::new("‹").small())
                    .on_hover_text("Previous cards")
                    .clicked()
                {
                    offset = (offset - (card_width + 16.) * card_columns as f32).max(0.);
                }
            });
        }
    });
    ui.add_space(12.);
    if all_tracks {
        let columns = (available / 360.).floor().clamp(1., 3.) as usize;
        let width = (available - (columns - 1) as f32 * 14.) / columns as f32;
        let tracks = shelf
            .entries
            .iter()
            .filter_map(|e| e.track.clone())
            .collect::<Vec<_>>();
        egui::Grid::new(("songs", columns, width.round() as u32))
            .spacing([14., 8.])
            .show(ui, |ui| {
                for (i, entry) in shelf.entries.iter().enumerate() {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(width, 76.), egui::Sense::click());
                    ui.painter().rect_filled(
                        rect,
                        10,
                        theme.color(if response.hovered() {
                            ColorRole::RowHover
                        } else {
                            ColorRole::Surface
                        }),
                    );
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect.shrink(10.))
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    child.set_clip_rect(ui.clip_rect().intersect(rect));
                    rows::artwork(&mut child, theme, entry.artwork.as_deref(), 56.);
                    child.vertical(|ui| {
                        ui.add(
                            egui::Label::new(theme.label(TextRole::Body, &entry.title).strong())
                                .truncate(),
                        );
                        ui.add(
                            egui::Label::new(
                                theme.secondary_label(TextRole::Caption, &entry.subtitle),
                            )
                            .truncate(),
                        );
                    });
                    if response.clicked() {
                        out.push(Action::ContextPlayed {
                            tracks: tracks.clone(),
                            start: i,
                        });
                    }
                    if let Some(track) = &entry.track
                        && let Some(action) = rows::row_context_menu(&response, track, context)
                    {
                        out.push(action);
                    }
                    response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(format!("Play {}", entry.title));
                    if (i + 1) % columns == 0 {
                        ui.end_row();
                    }
                }
            });
    } else {
        // Equal-width artwork cards expand to fit the viewport. Extra cards
        // remain reachable horizontally without making an entire shelf taller.
        let width = card_width;
        let scroll = egui::ScrollArea::horizontal()
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .horizontal_scroll_offset(offset)
            .id_salt("cards")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.;
                    for entry in &shelf.entries {
                        let response = if shelf.title.to_lowercase().contains("artists") {
                            artist_card(ui, width, entry, theme)
                        } else {
                            home::card(
                                ui,
                                width,
                                &entry.title,
                                &entry.subtitle,
                                entry.artwork.as_deref(),
                                theme,
                            )
                        };
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
            });
        ui.data_mut(|data| data.insert_temp(scroll_id, scroll.state.offset.x));
    }
    ui.add_space(28.);
}
fn artist_card(ui: &mut Ui, width: f32, entry: &Entry, theme: &dyn Theme) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, width + 60.), egui::Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect.expand(5.), 10, theme.color(ColorRole::RowHover));
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    if let Some(url) = &entry.artwork {
        child.add(
            egui::Image::new(url)
                .fit_to_exact_size(vec2(width, width))
                .corner_radius((width / 2.).min(255.) as u8),
        );
    } else {
        let (art, _) = child.allocate_exact_size(vec2(width, width), egui::Sense::hover());
        child.painter().circle_filled(
            art.center(),
            width / 2.,
            theme.color(ColorRole::ArtPlaceholder),
        );
    }
    child.add_space(6.);
    child.add(egui::Label::new(theme.label(TextRole::Body, &entry.title).strong()).truncate());
    child.add(
        egui::Label::new(theme.secondary_label(TextRole::Caption, &entry.subtitle)).truncate(),
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(&entry.title)
}
fn open(entry: &Entry, out: &mut Vec<Action>) {
    if let Some(track) = &entry.track {
        out.push(Action::ContextPlayed {
            tracks: vec![track.clone()],
            start: 0,
        });
    } else {
        out.push(Action::DiscoveryOpened(entry.clone()));
    }
}
pub fn collection(
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
                && ui
                    .add(super::components::primary(theme, "▶ Play"))
                    .clicked()
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

pub fn shelf_page(
    ui: &mut Ui,
    state: &State,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    let Page::DiscoveryShelf(shelf) = &state.page else {
        return;
    };
    super::components::page_title(ui, theme, &shelf.title);
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

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn offscreen_artwork_shelf_keeps_its_height() {
        let state = State::default();
        let context = super::super::row_context(&state);
        let shelf = Shelf {
            title: "Mixes".into(),
            entries: (0..8)
                .map(|i| Entry {
                    title: format!("Mix {i}"),
                    subtitle: String::new(),
                    artwork: None,
                    target: Target::Browse {
                        id: i.to_string(),
                        params: None,
                    },
                    track: None,
                })
                .collect(),
        };
        let ctx = egui::Context::default();
        let mut bottom = 0.;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(700., 300.),
                )),
                ..Default::default()
            },
            |ui| {
                ui.add_space(250.);
                shelf_view(
                    ui,
                    &shelf,
                    &crate::theme::DefaultTheme,
                    &context,
                    &mut vec![],
                );
                bottom = ui.cursor().top();
            },
        );
        output.textures_delta.clear();
        assert!(bottom > 550., "offscreen shelf collapsed to {bottom}");
        assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect.width() > 100. && r.rect.height() > 0. && r.rect.height() < 12.)), "artwork shelf paints an unnecessary horizontal scrollbar");
    }
}
