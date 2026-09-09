//! Ported from fastpotify (MIT, Copyright (c) 2026 Carmine Paolino),
//! src/ui/winamp/playlist.rs.
//!
//! The playlist window, joined to the bottom of the main one. Draws
//! `pledit.bmp`'s frame and shows the queue: the current track first,
//! then `Queue::upcoming`. A double click on a row plays it. Rows are
//! drawn with the bundled face (`pixel_text`), not the skin's bitmap
//! font, since track titles carry scripts the bitmap font does not.

#[cfg(test)]
use std::collections::HashSet;
use std::time::Duration;

use egui::{Color32, Sense, Ui, ViewportCommand};

use crate::core::action::Action;
use crate::core::model::Track;
use crate::core::state::{Page, State};
use crate::skin::layout::{self, Area};
use crate::skin::{PlaylistStyle, sprites};

use super::pixel_text::PixelText;
use super::{View, WinampShell, format_minutes_seconds, menu};
use crate::ui::queue_edit::Editor;

/// One line of the list.
struct Row {
    label: String,
    duration: Option<Duration>,
    current: bool,
    /// The row's index in `Queue::upcoming`, for the action that
    /// jumps there on a double click. `None` for the current track,
    /// which a double click does nothing to.
    jump_to: Option<usize>,
}

/// The whole window, `shell.playlist_height` skin pixels tall unless
/// shaded, drawn into a view whose origin is the window's top left.
pub(super) fn show(
    view: &mut View,
    ctx: &egui::Context,
    state: &State,
    shell: &mut WinampShell,
    out: &mut Vec<Action>,
    focused: bool,
) {
    if shell.playlist_shade {
        shade(view, ctx, shell, focused);
        return;
    }
    let height = shell
        .playlist_height
        .clamp(layout::PLAYLIST_MIN_HEIGHT, layout::PLAYLIST_MAX_HEIGHT);
    frame(view, height, focused);
    title_bar(view, ctx, shell);
    let rows = rows(state);
    let mut editor = Editor::load(ctx, &state.playback.queue);
    list(view, ctx, shell, &rows, height, out, &mut editor);
    scrollbar(view, shell, rows.len(), height);
    grip(view, shell, height);
    times(view, state, &rows, height);
    menus(view, state, shell, &rows, height, out, &mut editor);
    editor.keyboard(view.ui, state.playback.queue.upcoming().count(), out);
    editor.store(ctx);
}

/// The title bar: drags the viewport, and a double click shades the
/// window. The close and shade buttons live in the frame's own
/// bitmap; a lamp only draws over them while held.
fn title_bar(view: &mut View, ctx: &egui::Context, shell: &mut WinampShell) {
    let title = view.interact(
        Area::new(0, 0, layout::WINDOW_WIDTH, layout::PLAYLIST_TITLE_HEIGHT),
        "playlist-title",
        Sense::click_and_drag(),
    );
    if title.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if title.double_clicked() {
        shell.playlist_shade = !shell.playlist_shade;
    }
    if view
        .lamp_button(
            layout::PLAYLIST_CLOSE,
            sprites::PLAYLIST_CLOSE_PRESSED,
            "playlist-close",
        )
        .clicked()
    {
        shell.playlist_open = false;
    }
    if view
        .lamp_button(
            layout::PLAYLIST_SHADE,
            sprites::PLAYLIST_SHADE_PRESSED,
            "playlist-shade",
        )
        .clicked()
    {
        shell.playlist_shade = true;
    }
}

/// The playlist rolled up to its title bar: a plain bar with the
/// close and unshade buttons, and nothing else, since the shaded
/// main window already carries the song's name and time.
fn shade(view: &mut View, ctx: &egui::Context, shell: &mut WinampShell, focused: bool) {
    let width = layout::WINDOW_WIDTH;
    let height = layout::PLAYLIST_SHADE_HEIGHT;
    view.sprite_at(sprites::PLAYLIST_SHADE_LEFT, 0, 0);
    let mut x = 25;
    while x < width - 50 {
        view.sprite_clipped(
            sprites::PLAYLIST_SHADE_TILE,
            x,
            0,
            Area::new(25, 0, width - 75, height),
        );
        x += 25;
    }
    let right = if focused {
        sprites::PLAYLIST_SHADE_RIGHT_ACTIVE
    } else {
        sprites::PLAYLIST_SHADE_RIGHT
    };
    view.sprite_at(right, width - 50, 0);
    let title = view.interact(
        Area::new(0, 0, width, height),
        "playlist-shade-title",
        Sense::click_and_drag(),
    );
    if title.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if title.double_clicked() {
        shell.playlist_shade = false;
    }
    if view
        .lamp_button(
            layout::PLAYLIST_SHADE,
            sprites::PLAYLIST_UNSHADE_PRESSED,
            "playlist-unshade",
        )
        .clicked()
    {
        shell.playlist_shade = false;
    }
    if view
        .lamp_button(
            layout::PLAYLIST_CLOSE,
            sprites::PLAYLIST_CLOSE_PRESSED,
            "playlist-shade-close",
        )
        .clicked()
    {
        shell.playlist_open = false;
    }
}

/// The frame: corners and tiles across the top, tiles down the sides
/// however tall the list is, and the two halves of the bottom.
fn frame(view: &mut View, height: u32, focused: bool) {
    use sprites::*;
    let (top_left, title, top_tile, top_right) = if focused {
        (
            PLAYLIST_TOP_LEFT_ACTIVE,
            PLAYLIST_TITLE_ACTIVE,
            PLAYLIST_TOP_TILE_ACTIVE,
            PLAYLIST_TOP_RIGHT_ACTIVE,
        )
    } else {
        (
            PLAYLIST_TOP_LEFT,
            PLAYLIST_TITLE,
            PLAYLIST_TOP_TILE,
            PLAYLIST_TOP_RIGHT,
        )
    };
    let tile = layout::PLAYLIST_TILE_WIDTH;
    let width = layout::WINDOW_WIDTH;
    let inner = width - 2 * tile - 100;
    let left = inner / 2;
    let right = inner - left;
    view.sprite_at(top_left, 0, 0);
    let left_run = Area::new(tile, 0, left, layout::PLAYLIST_TITLE_HEIGHT);
    let mut x = tile;
    while x < tile + left {
        view.sprite_clipped(top_tile, x, 0, left_run);
        x += tile;
    }
    view.sprite_at(title, tile + left, 0);
    let right_run = Area::new(tile + left + 100, 0, right, layout::PLAYLIST_TITLE_HEIGHT);
    let mut x = tile + left + 100;
    while x < width - tile {
        view.sprite_clipped(top_tile, x, 0, right_run);
        x += tile;
    }
    view.sprite_at(top_right, width - tile, 0);

    let middle = Area::new(
        0,
        layout::PLAYLIST_TITLE_HEIGHT,
        layout::WINDOW_WIDTH,
        height - layout::PLAYLIST_TITLE_HEIGHT - layout::PLAYLIST_BOTTOM_HEIGHT,
    );
    let mut y = middle.y;
    while y < middle.y + middle.height {
        view.sprite_clipped(PLAYLIST_LEFT_TILE, 0, y, middle);
        view.sprite_clipped(
            PLAYLIST_RIGHT_TILE,
            layout::WINDOW_WIDTH - layout::PLAYLIST_RIGHT_WIDTH,
            y,
            middle,
        );
        y += layout::PLAYLIST_TILE_HEIGHT;
    }
    let bottom = height - layout::PLAYLIST_BOTTOM_HEIGHT;
    view.sprite_at(PLAYLIST_BOTTOM_LEFT, 0, bottom);
    view.sprite_at(PLAYLIST_BOTTOM_RIGHT, 125, bottom);
}

/// The list's own area, between the frame's tiles.
fn list_area(height: u32) -> Area {
    Area::new(
        layout::PLAYLIST_LEFT_WIDTH,
        layout::PLAYLIST_TITLE_HEIGHT,
        layout::WINDOW_WIDTH - layout::PLAYLIST_LEFT_WIDTH - layout::PLAYLIST_RIGHT_WIDTH,
        height - layout::PLAYLIST_TITLE_HEIGHT - layout::PLAYLIST_BOTTOM_HEIGHT,
    )
}

/// How many whole rows fit the list area at this height.
fn rows_visible(height: u32) -> usize {
    (list_area(height).height / layout::PLAYLIST_TRACK_HEIGHT) as usize
}

/// The row indices a scroll offset shows, for a list `total` rows
/// long at this height: never past the end, even when `scroll` is.
fn visible_row_range(scroll: usize, total: usize, height: u32) -> std::ops::Range<usize> {
    let visible = rows_visible(height);
    let start = scroll.min(total);
    let end = (start + visible).min(total);
    start..end
}

/// "1. artist - title", or "1. title" with no artist credit, the way
/// Winamp numbered a playlist.
fn row_text(number: usize, track: &Track) -> String {
    if track.artists.is_empty() {
        format!("{number}. {}", track.title)
    } else {
        format!("{number}. {} - {}", track.artist_names(), track.title)
    }
}

/// The playlist's rows: the current track, then the queue's upcoming
/// tracks in play order.
fn rows(state: &State) -> Vec<Row> {
    let queue = &state.playback.queue;
    let mut rows = Vec::new();
    if let Some(track) = queue.current() {
        rows.push(Row {
            label: row_text(1, track),
            duration: state.playback.track_duration.or(track.duration),
            current: true,
            jump_to: None,
        });
    }
    for (index, track) in queue.upcoming().enumerate() {
        rows.push(Row {
            label: row_text(rows.len() + 1, track),
            duration: track.duration,
            current: false,
            jump_to: Some(index),
        });
    }
    rows
}

fn rgb(color: [u8; 3]) -> Color32 {
    Color32::from_rgb(color[0], color[1], color[2])
}

/// The rows, in the skin's playlist colours.
#[allow(clippy::too_many_arguments)]
fn list(
    view: &mut View,
    ctx: &egui::Context,
    shell: &mut WinampShell,
    rows: &[Row],
    height: u32,
    out: &mut Vec<Action>,
    editor: &mut Editor,
) {
    let area = list_area(height);
    view.fill(
        area.x,
        area.y,
        area.width,
        area.height,
        rgb(view.skin.playlist.normal_background),
    );
    let visible = rows_visible(height);
    let most = rows.len().saturating_sub(visible);
    scroll_with_wheel(view, shell, area, visible, most);
    shell.playlist_scroll = shell.playlist_scroll.min(most);
    let style = view.skin.playlist.clone();
    let range = visible_row_range(shell.playlist_scroll, rows.len(), height);
    for (offset, index) in range.enumerate() {
        draw_row(
            view,
            ctx,
            shell,
            &style,
            &rows[index],
            index,
            offset,
            area,
            out,
            editor,
        );
    }
}

/// Scrolls the list from the mouse wheel while the pointer sits over
/// it: a notch moves three rows, a trackpad's points move a row per
/// row's height, a page moves the rows in view.
fn scroll_with_wheel(
    view: &mut View,
    shell: &mut WinampShell,
    area: Area,
    visible: usize,
    most: usize,
) {
    let hovered = view
        .interact(area, "playlist-list", Sense::hover())
        .contains_pointer();
    if !hovered {
        return;
    }
    let row_points = layout::PLAYLIST_TRACK_HEIGHT as f32 * view.unit;
    let (lines, points, pages) = view.ui.ctx().input(|input| wheel_deltas(&input.events));
    shell.playlist_wheel += rows_for_wheel(lines, points, pages, row_points, visible);
    let moved = shell.playlist_wheel.trunc();
    if moved == 0.0 {
        return;
    }
    shell.playlist_wheel -= moved;
    let scroll = shell.playlist_scroll as i64 + moved as i64;
    shell.playlist_scroll = scroll.clamp(0, most as i64) as usize;
}

/// This frame's wheel motion, summed by unit: notches, points, pages.
fn wheel_deltas(events: &[egui::Event]) -> (f32, f32, f32) {
    events
        .iter()
        .fold((0.0, 0.0, 0.0), |sum, event| match event {
            egui::Event::MouseWheel { unit, delta, .. } => match unit {
                egui::MouseWheelUnit::Line => {
                    let notch = if delta.y.abs() >= 1.0 {
                        delta.y.signum()
                    } else {
                        delta.y
                    };
                    (sum.0 + notch, sum.1, sum.2)
                }
                egui::MouseWheelUnit::Point => (sum.0, sum.1 + delta.y, sum.2),
                egui::MouseWheelUnit::Page => (sum.0, sum.1, sum.2 + delta.y),
            },
            _ => sum,
        })
}

/// How many rows a frame's wheel moves the list, positive downward. A
/// notch scrolls three rows, the Windows default Winamp's list
/// followed. egui's deltas are positive when the content moves down,
/// which is scrolling up.
fn rows_for_wheel(lines: f32, points: f32, pages: f32, row_points: f32, visible: usize) -> f32 {
    -(lines * 3.0 + points / row_points + pages * visible as f32)
}

/// One row: its selection background, its click and double-click
/// behaviour, and its text.
#[allow(clippy::too_many_arguments)]
fn draw_row(
    view: &mut View,
    ctx: &egui::Context,
    shell: &mut WinampShell,
    style: &PlaylistStyle,
    row: &Row,
    index: usize,
    offset: usize,
    area: Area,
    out: &mut Vec<Action>,
    editor: &mut Editor,
) {
    let line = Area::new(
        area.x,
        area.y + offset as u32 * layout::PLAYLIST_TRACK_HEIGHT,
        area.width,
        layout::PLAYLIST_TRACK_HEIGHT,
    );
    let rect = view.rect(line);
    let response = view.ui.interact(
        rect,
        egui::Id::new(("playlist-row", index)),
        Sense::click_and_drag(),
    );
    if row
        .jump_to
        .is_some_and(|index| editor.selected.contains(&index))
    {
        view.fill(
            line.x,
            line.y,
            line.width,
            line.height,
            rgb(style.selected_background),
        );
    }
    let color = if row.current {
        style.current
    } else {
        style.normal
    };
    draw_row_text(view, ctx, &mut shell.playlist_text, row, line, rgb(color));
    if let Some(index) = row.jump_to {
        editor.row(view.ui, &response, index, out);
        menu(
            egui::Popup::context_menu(&response),
            view.skin,
            view.unit,
            |ui| editor.menu(ui, index, out),
        );
    }
}

/// A click selects a row alone; a modified click adds it to, or drops
/// it from, the standing selection.
#[cfg(test)]
fn toggle_selection(selection: &mut HashSet<usize>, index: usize, adding: bool) {
    if adding {
        if !selection.remove(&index) {
            selection.insert(index);
        }
        return;
    }
    selection.clear();
    selection.insert(index);
}

/// The row's title, elided to fit, and its duration, right-aligned.
fn draw_row_text(
    view: &mut View,
    ctx: &egui::Context,
    text: &mut PixelText,
    row: &Row,
    line: Area,
    tint: Color32,
) {
    let pad = 3u32;
    let duration = duration_text(row.duration);
    let duration_line = text.line(ctx, &duration);
    let duration_at = Area::new(
        line.x + line.width.saturating_sub(pad + duration_line.width),
        line.y + line.height.saturating_sub(duration_line.height) / 2,
        duration_line.width,
        duration_line.height,
    );
    paint_line(view, duration_line.texture.id(), duration_at, 1.0, tint);

    let title_room = line.width.saturating_sub(3 * pad + duration_line.width);
    let title = fit_text(text, &row.label, title_room as f32);
    let title_line = text.line(ctx, &title);
    let title_at = Area::new(
        line.x + pad,
        line.y + line.height.saturating_sub(title_line.height) / 2,
        title_line.width.min(title_room),
        title_line.height,
    );
    let uv_right = title_at.width as f32 / title_line.width.max(1) as f32;
    paint_line(view, title_line.texture.id(), title_at, uv_right, tint);
}

/// Paints a rasterised line's texture, tinted, at `area`, showing
/// only up to `uv_right` of its width (1.0 for the whole line).
fn paint_line(view: &View, texture: egui::TextureId, area: Area, uv_right: f32, tint: Color32) {
    let clip = view.rect(area).intersect(view.ui.clip_rect());
    let painter = view.ui.painter().with_clip_rect(clip);
    painter.image(
        texture,
        view.rect(area),
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(uv_right, 1.0)),
        tint,
    );
}

/// A label cut to fit a width, with an ellipsis when it had to be.
fn fit_text(text: &mut PixelText, label: &str, width: f32) -> String {
    if text.width(label) <= width {
        return label.to_string();
    }
    let chars: Vec<char> = label.chars().collect();
    let mut keep = chars.len();
    while keep > 0 {
        keep -= 1;
        let candidate: String = chars[..keep].iter().collect::<String>() + "\u{2026}";
        if text.width(&candidate) <= width {
            return candidate;
        }
    }
    "\u{2026}".to_string()
}

fn duration_text(duration: Option<Duration>) -> String {
    duration
        .map(format_minutes_seconds)
        .unwrap_or_else(|| "--:--".into())
}

/// The handle in the right-hand tiles, dragged to scroll.
fn scrollbar(view: &mut View, shell: &mut WinampShell, total: usize, height: u32) {
    let visible = rows_visible(height);
    let most = total.saturating_sub(visible);
    let top = layout::PLAYLIST_TITLE_HEIGHT;
    let travel = height
        .saturating_sub(layout::PLAYLIST_TITLE_HEIGHT + layout::PLAYLIST_BOTTOM_HEIGHT)
        .saturating_sub(layout::PLAYLIST_SCROLL_HANDLE_HEIGHT);
    let fraction = scroll_fraction(shell.playlist_scroll, most);
    let y = top + (fraction * travel as f32).round() as u32;
    let handle = Area::new(
        layout::PLAYLIST_SCROLL_X,
        y,
        8,
        layout::PLAYLIST_SCROLL_HANDLE_HEIGHT,
    );
    let response = view.interact(handle, "playlist-scroll", Sense::click_and_drag());
    if response.dragged()
        && most > 0
        && let Some(pos) = response.interact_pointer_pos()
    {
        let pointer = (pos.y - view.origin.y) / view.unit
            - top as f32
            - layout::PLAYLIST_SCROLL_HANDLE_HEIGHT as f32 / 2.0;
        let fraction = (pointer / travel as f32).clamp(0.0, 1.0);
        shell.playlist_scroll = (fraction * most as f32).round() as usize;
    }
    let sprite = if response.dragged() || response.is_pointer_button_down_on() {
        sprites::PLAYLIST_SCROLL_HANDLE_PRESSED
    } else {
        sprites::PLAYLIST_SCROLL_HANDLE
    };
    view.sprite(sprite, handle);
}

/// The scrollbar thumb's position along its travel, as a fraction
/// from 0 to 1. A list with nothing to scroll sits at the top.
fn scroll_fraction(scroll: usize, most: usize) -> f32 {
    if most == 0 {
        0.0
    } else {
        scroll as f32 / most as f32
    }
}

/// The corner that stretches the list, a tile row at a time.
fn grip(view: &mut View, shell: &mut WinampShell, height: u32) {
    let corner = Area::new(
        layout::WINDOW_WIDTH - layout::PLAYLIST_GRIP,
        height - layout::PLAYLIST_GRIP,
        layout::PLAYLIST_GRIP,
        layout::PLAYLIST_GRIP,
    );
    let response = view
        .interact(corner, "playlist-grip", Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    if response.dragged() {
        shell.playlist_resize += response.drag_delta().y / view.unit;
        let (new_height, remainder) = grip_resized(shell.playlist_resize, height);
        shell.playlist_resize = remainder;
        shell.playlist_height = new_height;
    }
    if response.drag_stopped() {
        shell.playlist_resize = 0.0;
    }
}

/// One step of the resize grip's drag math: how much of `accumulated`
/// skin pixels of drag turn into whole `PLAYLIST_RESIZE_STEP` steps
/// on `current_height`, clamped to the playlist's own bounds, and
/// what is left over for the next frame.
fn grip_resized(accumulated: f32, current_height: u32) -> (u32, f32) {
    let step = layout::PLAYLIST_RESIZE_STEP as f32;
    let steps = (accumulated / step).trunc();
    if steps == 0.0 {
        return (current_height, accumulated);
    }
    let wanted = (current_height as i64 + steps as i64 * step as i64).clamp(
        layout::PLAYLIST_MIN_HEIGHT as i64,
        layout::PLAYLIST_MAX_HEIGHT as i64,
    ) as u32;
    (wanted, accumulated - steps * step)
}

/// The current track's position over its duration on the left, and
/// the queue's total upcoming duration on the right.
fn times(view: &mut View, state: &State, rows: &[Row], height: u32) {
    let bottom = height - layout::PLAYLIST_BOTTOM_HEIGHT;
    let position = state.playback.position;
    let duration = state.playback.track_duration.unwrap_or_default();
    let elapsed = position_over_duration_text(position, duration);
    let (x, dy) = layout::PLAYLIST_RUNNING_TIME;
    view.text(
        &elapsed,
        Area::new(x, bottom + dy, 5 * elapsed.len() as u32, 6),
    );

    let total = total_upcoming_duration(rows);
    let total_text = duration_text(total);
    let (x, dy) = layout::PLAYLIST_TRACK_TIME;
    view.text(
        &total_text,
        Area::new(x, bottom + dy, 5 * total_text.len() as u32, 6),
    );
}

fn position_over_duration_text(position: Duration, duration: Duration) -> String {
    format!(
        "{}/{}",
        format_minutes_seconds(position),
        format_minutes_seconds(duration)
    )
}

fn total_upcoming_duration(rows: &[Row]) -> Option<Duration> {
    rows.iter()
        .filter(|row| !row.current)
        .map(|row| row.duration)
        .sum()
}

/// The five bottom menus: ADD, REM, SEL, MISC, and LIST.
#[allow(clippy::too_many_arguments)]
fn menus(
    view: &mut View,
    state: &State,
    _shell: &mut WinampShell,
    _rows: &[Row],
    height: u32,
    out: &mut Vec<Action>,
    editor: &mut Editor,
) {
    let bottom = height - layout::PLAYLIST_BOTTOM_HEIGHT;
    let unit = view.unit;
    for (name, x) in layout::PLAYLIST_MENUS {
        let area = Area::new(x, bottom + 8, 22, 18);
        let button = view
            .interact(area, &format!("playlist-menu-{name}"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        menu(
            egui::Popup::menu(&button),
            view.skin,
            unit,
            |ui| match name {
                "add" => add_menu(ui, out),
                "rem" => {
                    if ui
                        .add_enabled(
                            !editor.selected.is_empty(),
                            egui::Button::new("Remove selected"),
                        )
                        .clicked()
                    {
                        editor.remove(out);
                        ui.close();
                    }
                    rem_menu(ui, out);
                }
                "sel" => {
                    if ui.button("Select all").clicked() {
                        editor
                            .selected
                            .extend(0..state.playback.queue.upcoming().count());
                    }
                    if ui.button("Select none").clicked() {
                        editor.selected.clear();
                    }
                }
                "misc" => misc_menu(ui, state, out),
                _ => list_menu(ui),
            },
        );
    }
}

/// Opens the main window's Search page, the only place ytamp adds
/// songs from.
fn add_menu(ui: &mut Ui, out: &mut Vec<Action>) {
    crate::ui::local_files::add_button(ui, out);
    if ui.button("Search").clicked() {
        out.push(Action::WinampToggled);
        out.push(Action::NavigatedTo(Page::Search));
    }
}

fn rem_menu(ui: &mut Ui, out: &mut Vec<Action>) {
    if ui.button("Remove all queued songs").clicked() {
        out.push(Action::QueueCleared);
    }
}

/// The current track's album and artist pages.
fn misc_menu(ui: &mut Ui, state: &State, out: &mut Vec<Action>) {
    let Some(track) = state.playback.queue.current() else {
        ui.add_enabled(false, egui::Button::new("Album"));
        return;
    };
    if let Some(album_id) = &track.album_id
        && ui.button("Album").clicked()
    {
        out.push(Action::WinampToggled);
        out.push(Action::AlbumOpened(album_id.clone()));
    }
    if let Some(artist) = track.artists.first()
        && ui.button("Artist").clicked()
    {
        out.push(Action::WinampToggled);
        out.push(Action::ArtistLinkOpened(artist.clone()));
    }
}

fn list_menu(ui: &mut Ui) {
    ui.add_enabled(false, egui::Button::new("Not available yet"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, duration_secs: Option<u64>) -> Track {
        Track {
            source: Default::default(),
            id: crate::core::model::TrackId(id.into()),
            title: id.into(),
            artists: vec![crate::core::model::ArtistRef::named("Artist")],
            album: None,
            album_id: None,
            duration: duration_secs.map(Duration::from_secs),
            thumbnail_url: None,
            playlist_item_id: None,
        }
    }

    #[test]
    fn decoded_duration_is_shown_in_the_current_playlist_row() {
        let mut state = State::default();
        crate::core::update::update(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![track("amethyst", None)],
                start: 0,
            },
            &mut |_| 0,
        );
        crate::core::update::update(
            &mut state,
            Action::Player(crate::core::action::PlayerEvent::TrackStarted {
                duration: Some(Duration::from_secs(450)),
                channels: 2,
                sample_rate: 44100,
            }),
            &mut |_| 0,
        );
        assert_eq!(duration_text(rows(&state)[0].duration), "7:30");
    }

    #[test]
    fn rows_are_numbered_the_way_winamp_did() {
        let mut with_artist = track("a", None);
        with_artist.title = "Rosewood".into();
        with_artist.artists = vec![crate::core::model::ArtistRef::named("Bonobo")];
        assert_eq!(row_text(1, &with_artist), "1. Bonobo - Rosewood");

        let mut no_artist = track("b", None);
        no_artist.title = "Episode 12".into();
        no_artist.artists = vec![];
        assert_eq!(row_text(12, &no_artist), "12. Episode 12");
    }

    #[test]
    fn the_list_holds_whole_rows_only() {
        assert_eq!(rows_visible(116), 4);
        assert_eq!(rows_visible(174), 8);
        assert_eq!(list_area(174), Area::new(12, 20, 243, 116));
    }

    #[test]
    fn the_visible_range_never_runs_past_the_end() {
        assert_eq!(visible_row_range(0, 10, 116), 0..4);
        assert_eq!(visible_row_range(2, 10, 116), 2..6);
        // A scroll offset past the end clamps, and shows nothing.
        assert_eq!(visible_row_range(50, 10, 116), 10..10);
    }

    #[test]
    fn a_notch_scrolls_three_rows_as_windows_did() {
        assert_eq!(rows_for_wheel(-1.0, 0.0, 0.0, 20.0, 8), 3.0);
        assert_eq!(rows_for_wheel(2.0, 0.0, 0.0, 20.0, 8), -6.0);
        assert_eq!(rows_for_wheel(0.0, -40.0, 0.0, 20.0, 8), 2.0);
        assert_eq!(rows_for_wheel(0.0, 0.0, -1.0, 20.0, 8), 8.0);
    }

    #[test]
    fn toggle_selection_replaces_unless_adding() {
        let mut selection = HashSet::new();
        toggle_selection(&mut selection, 1, false);
        assert_eq!(selection, HashSet::from([1]));
        toggle_selection(&mut selection, 2, true);
        assert_eq!(selection, HashSet::from([1, 2]));
        toggle_selection(&mut selection, 2, true);
        assert_eq!(selection, HashSet::from([1]));
        toggle_selection(&mut selection, 3, false);
        assert_eq!(selection, HashSet::from([3]));
    }

    #[test]
    fn missing_duration_is_not_displayed_as_zero() {
        assert_eq!(duration_text(Some(Duration::from_secs(65))), "1:05");
        assert_eq!(duration_text(None), "--:--");
    }

    #[test]
    fn position_over_duration_reads_minutes_and_seconds() {
        assert_eq!(
            position_over_duration_text(Duration::from_secs(65), Duration::from_secs(251)),
            "1:05/4:11"
        );
    }

    #[test]
    fn total_upcoming_duration_skips_the_current_track() {
        let rows = vec![
            Row {
                label: "1. a".into(),
                duration: Some(Duration::from_secs(100)),
                current: true,
                jump_to: None,
            },
            Row {
                label: "2. b".into(),
                duration: Some(Duration::from_secs(60)),
                current: false,
                jump_to: Some(0),
            },
            Row {
                label: "3. c".into(),
                duration: Some(Duration::from_secs(40)),
                current: false,
                jump_to: Some(1),
            },
        ];
        assert_eq!(
            total_upcoming_duration(&rows),
            Some(Duration::from_secs(100))
        );
    }

    #[test]
    fn scroll_fraction_is_the_offset_over_the_scrollable_rows() {
        assert_eq!(scroll_fraction(0, 0), 0.0);
        assert_eq!(scroll_fraction(2, 4), 0.5);
    }

    #[test]
    fn grip_resized_steps_only_once_a_whole_step_accumulates() {
        assert_eq!(grip_resized(10.0, 116), (116, 10.0));
        assert_eq!(grip_resized(29.0, 116), (145, 0.0));
        assert_eq!(grip_resized(60.0, 116), (174, 2.0));
        // Shrinking clamps at the minimum height.
        assert_eq!(grip_resized(-29.0, 116), (116, 0.0));
        // Growing clamps at the maximum height; the step is still
        // spent, so reversing the drag needs a full step to shrink.
        assert_eq!(
            grip_resized(29.0, layout::PLAYLIST_MAX_HEIGHT),
            (layout::PLAYLIST_MAX_HEIGHT, 0.0)
        );
    }
}
