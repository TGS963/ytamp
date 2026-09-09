use super::{
    home,
    rows::{self, RowContext},
};
use crate::ui::components;
use crate::{
    core::{
        action::Action,
        discovery::{Entry, Shelf},
    },
    theme::{ColorRole, TextRole, Theme},
};
use egui::{Ui, vec2};

pub(crate) fn shelf_view(
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
    let offset = ui
        .data(|data| data.get_temp::<f32>(scroll_id))
        .unwrap_or(0.);
    let mut layout = ShelfLayout {
        all_tracks,
        card_columns,
        card_width,
        max_offset,
        scroll_id,
        offset,
    };
    render_shelf_header(ui, shelf, theme, out, &mut layout);
    ui.add_space(12.);
    if all_tracks {
        render_track_shelf(ui, shelf, theme, context, out, available);
    } else {
        render_card_shelf(ui, shelf, theme, context, out, &layout);
    }
    ui.add_space(28.);
}

struct ShelfLayout {
    all_tracks: bool,
    card_columns: usize,
    card_width: f32,
    max_offset: f32,
    scroll_id: egui::Id,
    offset: f32,
}

fn render_shelf_header(
    ui: &mut Ui,
    shelf: &Shelf,
    theme: &dyn Theme,
    out: &mut Vec<Action>,
    layout: &mut ShelfLayout,
) {
    ui.horizontal(|ui| {
        let title_width = (ui.available_width() - 170.).max(80.);
        ui.allocate_ui_with_layout(
            vec2(title_width, 32.),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(title_width);
                components::heading(ui, theme, &shelf.title);
            },
        );
        if ui.add(components::quiet("See all")).clicked() {
            out.push(Action::DiscoveryShelfOpened(shelf.clone()));
        }
        if !layout.all_tracks && shelf.entries.len() > layout.card_columns {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        layout.offset < layout.max_offset - 1.,
                        egui::Button::new("›").small(),
                    )
                    .on_hover_text("More in this shelf")
                    .clicked()
                {
                    layout.offset += (layout.card_width + 16.) * layout.card_columns as f32;
                }
                if ui
                    .add_enabled(layout.offset > 0., egui::Button::new("‹").small())
                    .on_hover_text("Previous cards")
                    .clicked()
                {
                    layout.offset = (layout.offset
                        - (layout.card_width + 16.) * layout.card_columns as f32)
                        .max(0.);
                }
            });
        }
    });
}

fn render_track_shelf(
    ui: &mut Ui,
    shelf: &Shelf,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
    available: f32,
) {
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
                        egui::Label::new(theme.secondary_label(TextRole::Caption, &entry.subtitle))
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
}

fn render_card_shelf(
    ui: &mut Ui,
    shelf: &Shelf,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
    layout: &ShelfLayout,
) {
    // Equal-layout.card_width artwork cards expand to fit the viewport. Extra cards
    // remain reachable horizontally without making an entire shelf taller.
    let scroll = egui::ScrollArea::horizontal()
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .horizontal_scroll_offset(layout.offset)
        .id_salt("cards")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.;
                for entry in &shelf.entries {
                    let response = if shelf.title.to_lowercase().contains("artists") {
                        artist_card(ui, layout.card_width, entry, theme)
                    } else {
                        home::card(
                            ui,
                            layout.card_width,
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
    ui.data_mut(|data| data.insert_temp(layout.scroll_id, scroll.state.offset.x));
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

pub(super) fn open(entry: &Entry, out: &mut Vec<Action>) {
    if let Some(track) = &entry.track {
        out.push(Action::ContextPlayed {
            tracks: vec![track.clone()],
            start: 0,
        });
    } else {
        out.push(Action::DiscoveryOpened(entry.clone()));
    }
}
