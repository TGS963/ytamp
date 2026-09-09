//! Ported from fastpotify (MIT, Copyright (c) 2026 Carmine Paolino), src/ui/winamp/mod.rs
//! and src/winamp.rs.
//!
//! The Winamp skin window: the main window's controls, drawn through
//! the skin the listener has on. `App` owns the [`WinampShell`] and
//! opens the window as an egui viewport; this module reads `&State`
//! and the shell, draws, and returns the actions the listener asked
//! for, the same shape as every other view in `ui`.

mod display;
mod equalizer;
mod menus;
mod pixel_text;
mod playlist;
mod shell;
mod transport;
mod view;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Color32, Sense, Ui, ViewportCommand};

use crate::core::action::Action;
use crate::core::queue::RepeatMode;
use crate::core::state::{PlayStatus, State};
use crate::skin::layout::{self, Area};
use crate::skin::{Sheet, Skin, sprites};
use crate::vis::{self, AudioTap};

pub(super) use display::{format_minutes_seconds, slider_active_value, slider_fraction};
pub use display::{seek_fraction, shown_duration, time_digits};
pub(super) use menus::menu;
pub use view::{SliderEvent, View};

/// How often the window repaints on its own while a track plays, so
/// the position and the blink move without a pointer event to wake it.
const PLAYING_REPAINT: Duration = Duration::from_millis(250);

/// How often the window repaints while the marquee scrolls: the
/// marquee's own step interval, so no step is ever skipped.
const MARQUEE_REPAINT: Duration = Duration::from_millis(220);

/// How often the visualiser wants a frame while it moves. Two of
/// these gives the analyser's own 16.667 ms step some room without
/// asking egui for an unreachable frame rate.
const VIS_FRAME: Duration = Duration::from_micros(16_667);

/// What the display shows: the spectrum bars, the oscilloscope, or
/// nothing. A click on the display moves to the next one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VisMode {
    #[default]
    Spectrum,
    Scope,
    Off,
}

impl VisMode {
    /// The mode a click on the display moves to.
    fn next(self) -> Self {
        match self {
            VisMode::Spectrum => VisMode::Scope,
            VisMode::Scope => VisMode::Off,
            VisMode::Off => VisMode::Spectrum,
        }
    }
}

/// The skin data and window-local look the core never sees: the
/// loaded skin, its textures on the graphics card, and the two toggles
/// Winamp kept out of its own settings file.
pub struct WinampShell {
    skin: Arc<Skin>,
    textures: HashMap<Sheet, egui::TextureHandle>,
    /// The viewport context the textures were built for. Textures
    /// belong to the context that made them, so a different one
    /// (a fresh window) means starting over.
    texture_ctx: Option<egui::Context>,
    /// Counts down instead of up; a click on the time toggles it.
    pub time_remaining: bool,
    /// The window rolled up to its title bar.
    pub shade: bool,
    /// The marquee's window onto its current text: what the text was
    /// last frame, how far it has scrolled, and when it last stepped.
    marquee_text: String,
    marquee_cursor: usize,
    marquee_last_step: f64,
    /// The last notice shown in the marquee, and when it started, so
    /// a new one shows once for a few seconds and then steps aside.
    shown_notice: Option<(String, f64)>,
    /// Whether the playlist window is attached below the main one.
    pub playlist_open: bool,
    pub equalizer_open: bool,
    pub equalizer_shade: bool,
    /// The playlist window's height in skin pixels, one of
    /// `PLAYLIST_MIN_HEIGHT` plus a multiple of `PLAYLIST_RESIZE_STEP`.
    pub playlist_height: u32,
    /// The playlist window rolled up to its title bar.
    pub playlist_shade: bool,
    /// How many rows the list has scrolled past.
    pub playlist_scroll: usize,
    /// A drag's leftover wheel and grip motion, carried to the next
    /// frame so a slow drag still steps once it adds up.
    playlist_wheel: f32,
    playlist_resize: f32,
    /// The playlist's own text cache: track rows drawn with the
    /// bundled face, not the skin's bitmap font.
    playlist_text: pixel_text::PixelText,
    /// The display's mode: spectrum bars, the oscilloscope, or off.
    pub vis_mode: VisMode,
    /// The spectrum analyser's falling bars and peaks, moved once
    /// each visualiser step regardless of how often the window
    /// repaints.
    vis_analyser: vis::Analyser,
}

impl Default for WinampShell {
    fn default() -> Self {
        Self {
            skin: Skin::builtin(),
            textures: HashMap::new(),
            texture_ctx: None,
            time_remaining: false,
            shade: false,
            marquee_text: String::new(),
            marquee_cursor: 0,
            marquee_last_step: 0.0,
            shown_notice: None,
            playlist_open: false,
            equalizer_open: false,
            equalizer_shade: false,
            playlist_height: layout::PLAYLIST_MIN_HEIGHT,
            playlist_shade: false,
            playlist_scroll: 0,
            playlist_wheel: 0.0,
            playlist_resize: 0.0,
            playlist_text: pixel_text::PixelText::default(),
            vis_mode: VisMode::default(),
            vis_analyser: vis::Analyser::default(),
        }
    }
}

/// The stack's height in skin pixels, pure so the arithmetic is
/// testable on its own.
fn stack_height(
    shade: bool,
    playlist_open: bool,
    playlist_shade: bool,
    playlist_height: u32,
) -> u32 {
    let mut height = window_height(shade);
    if playlist_open {
        height += if playlist_shade {
            layout::PLAYLIST_SHADE_HEIGHT
        } else {
            playlist_height.clamp(layout::PLAYLIST_MIN_HEIGHT, layout::PLAYLIST_MAX_HEIGHT)
        };
    }
    height
}

/// How many characters the marquee shows at once: thirty whole ones
/// and the edge of a thirty-first, the same window classic Winamp
/// used.
const MARQUEE_CHARS: usize = 31;
/// Text this long fits the marquee without scrolling.
const MARQUEE_FITS: usize = 30;
/// How long the marquee waits between one-character steps.
const MARQUEE_STEP_SECS: f64 = 0.220;
/// What separates the end of a scrolling text from its start again.
const MARQUEE_GAP: &str = "  ***  ";
/// How long a fresh notice holds the marquee before the usual text
/// returns.
const NOTICE_DURATION_SECS: f64 = 3.0;

/// Whether `text` is too long for the marquee to show whole, and so
/// needs to scroll.
fn marquee_scrolls(text: &str) -> bool {
    text.chars().count() > MARQUEE_FITS
}

/// The 31-character window onto `text` starting at `cursor`, wrapping
/// through the gap that marks the loop back to the start. `text`
/// shorter than the marquee shows whole, at any cursor.
fn marquee_window(text: &str, cursor: usize) -> String {
    if !marquee_scrolls(text) {
        return text.to_string();
    }
    let strip: Vec<char> = format!("{text}{MARQUEE_GAP}").chars().collect();
    (0..MARQUEE_CHARS)
        .map(|index| strip[(cursor + index) % strip.len()])
        .collect()
}

/// How many 220 ms steps have passed since the marquee last moved.
/// Zero when the clock has not advanced, or has gone backwards.
fn marquee_steps_since(now: f64, last_step: f64) -> usize {
    if now <= last_step {
        return 0;
    }
    ((now - last_step) / MARQUEE_STEP_SECS) as usize
}

/// Logical points per skin pixel: `scale` screen pixels, converted to
/// points for this display's density.
pub fn unit(scale: u8, pixels_per_point: f32) -> f32 {
    let pixels_per_point = if pixels_per_point > 0.0 {
        pixels_per_point
    } else {
        1.0
    };
    scale.clamp(1, 4) as f32 / pixels_per_point
}

/// The window's height in skin pixels: just the title bar in shade
/// mode, the whole main window otherwise.
pub fn window_height(shade: bool) -> u32 {
    if shade {
        layout::SHADE_HEIGHT
    } else {
        layout::WINDOW_HEIGHT
    }
}

/// The `.wsz` and `.zip` files dropped on this window this frame, each
/// as the action that installs it. `App` calls this for both the main
/// window and the skin window, since a skin can land on either.
pub fn dropped_skins(ctx: &egui::Context) -> Vec<Action> {
    ctx.input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .map(|file| file.path().to_path_buf())
            .filter(|path| is_skin_file(path))
            .map(Action::SkinFileDropped)
            .collect()
    })
}

/// Whether a dropped file could be a Winamp skin, by its name.
fn is_skin_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("wsz")
                || extension.eq_ignore_ascii_case("zip")
                || extension.eq_ignore_ascii_case("wal")
        })
}

/// The window's size in logical points, for `show_viewport_immediate`:
/// the main window, and the playlist window under it while it is open.
pub fn window_size_points(shell: &WinampShell, scale: u8, pixels_per_point: f32) -> egui::Vec2 {
    let unit = unit(scale, pixels_per_point);
    egui::vec2(layout::WINDOW_WIDTH as f32, shell.stack_height() as f32) * unit
}

/// How long a rejected resize waits before it retries, so a platform
/// that refuses the size is not asked again every frame.
const RESIZE_RETRY: f64 = 1.0;

/// Nudges an already-open window to `wanted`. The viewport builder's
/// own size only takes on the window's creation, so a later scale
/// change needs this: `App` still passes the fresh size to the
/// builder too, for the window's first frame.
///
/// Ported from fastpotify (MIT, Copyright (c) 2026 Carmine Paolino),
/// src/ui/winamp/mod.rs, `fit_window` (lines 97-116).
fn fit_window(ctx: &egui::Context, wanted: egui::Vec2) {
    let current = ctx.viewport_rect().size();
    if (current - wanted).abs().max_elem() < 1.0 {
        return;
    }
    let asked_id = egui::Id::new("winamp-fit-asked");
    let last_ask: Option<f64> = ctx.data(|data| data.get_temp(asked_id));
    let now = ctx.input(|input| input.time);
    if last_ask.is_some_and(|last_ask| now - last_ask < RESIZE_RETRY) {
        return;
    }
    ctx.data_mut(|data| data.insert_temp(asked_id, now));
    ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(wanted));
    ctx.send_viewport_cmd(ViewportCommand::MaxInnerSize(wanted));
    ctx.send_viewport_cmd(ViewportCommand::InnerSize(wanted));
}

/// Keeps the window's always-on-top state matched to the setting,
/// since the viewport builder's own version of this only takes on the
/// window's creation.
fn apply_on_top(ctx: &egui::Context, on_top: bool) {
    let level = if on_top {
        egui::WindowLevel::AlwaysOnTop
    } else {
        egui::WindowLevel::Normal
    };
    ctx.send_viewport_cmd(ViewportCommand::WindowLevel(level));
}

/// Draws the whole window and reads its controls, adding any action a
/// click or a drag produced to `out`.
pub fn show(ui: &mut Ui, state: &State, shell: &mut WinampShell, out: &mut Vec<Action>) {
    crate::ui::keyboard_shortcuts(ui, out);
    let ctx = ui.ctx().clone();
    let unit = unit(state.winamp.scale, ctx.pixels_per_point());
    fit_window(
        &ctx,
        window_size_points(shell, state.winamp.scale, ctx.pixels_per_point()),
    );
    apply_on_top(&ctx, state.winamp.on_top);
    let origin = ui.max_rect().min;
    let focused = ctx.input(|input| input.viewport().focused).unwrap_or(true);
    let time = ctx.input(|input| input.time);
    let textures = shell.textures(&ctx);
    let skin = shell.skin.clone();
    let mut below_y = window_height(shell.shade);
    let mut view = View {
        ui,
        origin,
        unit,
        skin: &skin,
        mask: if shell.shade {
            skin.regions.shade.as_ref()
        } else {
            skin.regions.normal.as_ref()
        },
        textures: &textures,
    };
    let vis_moving = if shell.shade {
        display::shade_bar(&mut view, &ctx, state, shell, out, focused);
        false
    } else {
        display::full_window(&mut view, &ctx, state, shell, out, focused, time)
    };
    if shell.equalizer_open {
        let mut below = View {
            ui: view.ui,
            origin: origin + egui::vec2(0., below_y as f32 * unit),
            unit,
            skin: &skin,
            mask: if shell.equalizer_shade {
                skin.regions.equalizer_shade.as_ref()
            } else {
                skin.regions.equalizer.as_ref()
            },
            textures: &textures,
        };
        equalizer::show(&mut below, state, shell, out, focused);
        below_y += if shell.equalizer_shade {
            layout::EQ_SHADE_HEIGHT
        } else {
            layout::EQ_HEIGHT
        };
    }
    if shell.playlist_open {
        let mut below = View {
            ui: view.ui,
            origin: origin + egui::vec2(0.0, below_y as f32 * unit),
            unit,
            skin: &skin,
            mask: None,
            textures: &textures,
        };
        playlist::show(&mut below, &ctx, state, shell, out, focused);
    }
    if vis_moving {
        ctx.request_repaint_after(VIS_FRAME * 2);
    } else if state.playback.status == PlayStatus::Playing
        || state.playback.status == PlayStatus::Paused
    {
        ctx.request_repaint_after(PLAYING_REPAINT);
    }
}

#[cfg(test)]
mod tests {
    use super::display::*;
    use super::*;

    #[test]
    fn scale_sets_points_per_skin_pixel_for_the_display() {
        assert_eq!(unit(2, 1.0), 2.0);
        assert_eq!(unit(2, 2.0), 1.0);
        assert_eq!(unit(4, 2.0), 2.0);
        // Out-of-range scales clamp, and a zero density falls back to 1x.
        assert_eq!(unit(9, 1.0), 4.0);
        assert_eq!(unit(2, 0.0), 2.0);
    }

    #[test]
    fn the_window_shrinks_to_the_title_bar_while_shaded() {
        assert_eq!(window_height(false), layout::WINDOW_HEIGHT);
        assert_eq!(window_height(true), layout::SHADE_HEIGHT);
        let mut shell = WinampShell::new();
        assert_eq!(window_size_points(&shell, 2, 1.0), egui::vec2(550.0, 232.0));
        shell.shade = true;
        assert_eq!(window_size_points(&shell, 1, 1.0), egui::vec2(275.0, 14.0));
    }

    #[test]
    fn the_stack_grows_by_the_open_playlists_height() {
        assert_eq!(
            stack_height(false, false, false, 116),
            layout::WINDOW_HEIGHT
        );
        assert_eq!(
            stack_height(false, true, false, 200),
            layout::WINDOW_HEIGHT + 200
        );
        assert_eq!(
            stack_height(false, true, true, 200),
            layout::WINDOW_HEIGHT + layout::PLAYLIST_SHADE_HEIGHT
        );
        // An out-of-range height clamps to the playlist's own bounds.
        assert_eq!(
            stack_height(false, true, false, 10_000),
            layout::WINDOW_HEIGHT + layout::PLAYLIST_MAX_HEIGHT
        );
    }

    #[test]
    fn time_digits_split_minutes_and_seconds_and_cap_at_ninety_nine() {
        assert_eq!(time_digits(Duration::from_secs(65)), [0, 1, 0, 5]);
        assert_eq!(time_digits(Duration::from_secs(0)), [0, 0, 0, 0]);
        assert_eq!(time_digits(Duration::from_secs(100 * 60)), [9, 9, 0, 0]);
    }

    #[test]
    fn shown_duration_counts_down_when_remaining_is_asked_for() {
        let position = Duration::from_secs(30);
        let duration = Duration::from_secs(200);
        assert_eq!(shown_duration(position, duration, false), position);
        assert_eq!(
            shown_duration(position, duration, true),
            Duration::from_secs(170)
        );
    }

    #[test]
    fn seek_fraction_is_the_position_over_the_duration() {
        assert_eq!(
            seek_fraction(Duration::from_secs(30), Duration::from_secs(120)),
            0.25
        );
        assert_eq!(seek_fraction(Duration::from_secs(30), Duration::ZERO), 0.0);
        assert_eq!(
            seek_fraction(Duration::from_secs(999), Duration::from_secs(120)),
            1.0
        );
    }

    #[test]
    fn a_committed_slider_value_wins_over_the_resting_position() {
        assert_eq!(slider_fraction(SliderEvent::None, 0.4), 0.4);
        assert_eq!(slider_fraction(SliderEvent::Dragging(0.7), 0.4), 0.7);
        assert_eq!(slider_fraction(SliderEvent::Committed(0.9), 0.4), 0.9);
    }

    #[test]
    fn the_blink_alternates_every_half_second() {
        assert!(!blinked_off(0.0));
        assert!(blinked_off(0.5));
        assert!(!blinked_off(1.0));
        assert!(blinked_off(1.6));
    }

    #[test]
    fn shade_time_text_shows_a_minus_only_when_counting_down() {
        assert_eq!(shade_time_text(Duration::from_secs(65), false), " 1:05");
        assert_eq!(shade_time_text(Duration::from_secs(65), true), "-1:05");
    }

    #[test]
    fn a_dropped_file_is_a_skin_by_its_extension_only() {
        assert!(is_skin_file(std::path::Path::new("/tmp/Zaxon.wsz")));
        assert!(is_skin_file(std::path::Path::new("/tmp/Zaxon.WSZ")));
        assert!(is_skin_file(std::path::Path::new("/tmp/Zaxon.zip")));
        assert!(!is_skin_file(std::path::Path::new("/tmp/readme.txt")));
        assert!(!is_skin_file(std::path::Path::new("/tmp/no-extension")));
    }

    #[test]
    fn short_text_never_scrolls_and_long_text_does() {
        assert!(!marquee_scrolls("ytamp"));
        assert!(!marquee_scrolls(&"x".repeat(30)));
        assert!(marquee_scrolls(&"x".repeat(31)));
    }

    #[test]
    fn a_short_marquee_window_is_the_text_itself() {
        assert_eq!(marquee_window("ytamp", 0), "ytamp");
        assert_eq!(marquee_window("ytamp", 5), "ytamp");
    }

    #[test]
    fn a_long_marquee_window_shows_thirty_one_characters_and_wraps_through_the_gap() {
        let text = "Radiohead - Everything In Its Right Place";
        let first = marquee_window(text, 0);
        assert_eq!(first.chars().count(), MARQUEE_CHARS);
        assert!(text.starts_with(&first));

        let strip_len = text.chars().count() + MARQUEE_GAP.chars().count();
        let wrapped = marquee_window(text, strip_len);
        assert_eq!(wrapped, first);
    }

    #[test]
    fn marquee_steps_advance_every_220_milliseconds() {
        assert_eq!(marquee_steps_since(0.1, 0.0), 0);
        assert_eq!(marquee_steps_since(0.22, 0.0), 1);
        assert_eq!(marquee_steps_since(0.65, 0.0), 2);
        // The clock going backwards, or standing still, steps nothing.
        assert_eq!(marquee_steps_since(0.0, 0.5), 0);
    }

    #[test]
    fn the_track_line_names_the_song_and_its_length_or_the_app_with_nothing_playing() {
        let mut state = State::default();
        assert_eq!(track_marquee_text(&state), "ytamp");

        let track = crate::core::model::Track {
            id: crate::core::model::TrackId("t1".into()),
            title: "Everything In Its Right Place".into(),
            artists: vec![crate::core::model::ArtistRef {
                name: "Radiohead".into(),
                id: None,
            }],
            album: None,
            album_id: None,
            duration: None,
            thumbnail_url: None,
            playlist_item_id: None,
        };
        state
            .playback
            .queue
            .play_context(vec![track], 0, &mut |_| 0);
        state.playback.track_duration = Some(Duration::from_secs(251));
        assert_eq!(
            track_marquee_text(&state),
            "Radiohead - Everything In Its Right Place (4:11)"
        );
    }

    #[test]
    fn the_marquee_priority_puts_a_live_slider_before_the_track_line() {
        let track_line = "Radiohead - Everything In Its Right Place (4:11)";
        assert_eq!(
            marquee_priority_text(track_line, Some(0.5), None, None),
            "VOLUME: 50%"
        );
        assert_eq!(
            marquee_priority_text(
                track_line,
                None,
                Some((Duration::from_secs(65), Duration::from_secs(251))),
                None
            ),
            "SEEK TO: 1:05/4:11"
        );
        assert_eq!(
            marquee_priority_text(track_line, None, None, Some("Sign-in failed")),
            "Sign-in failed"
        );
        assert_eq!(
            marquee_priority_text(track_line, None, None, None),
            track_line
        );
    }

    #[test]
    fn a_shell_shows_a_fresh_notice_for_a_few_seconds_then_steps_aside() {
        let mut shell = WinampShell::new();
        assert_eq!(shell.current_notice(&[], 0.0), None);

        let notices = ["a notice".to_string()];
        assert_eq!(
            shell.current_notice(&notices, 0.0),
            Some("a notice".to_string())
        );
        assert_eq!(
            shell.current_notice(&notices, 2.9),
            Some("a notice".to_string())
        );
        assert_eq!(shell.current_notice(&notices, 3.1), None);
    }

    #[test]
    fn the_vis_mode_cycles_spectrum_scope_off_and_back() {
        assert_eq!(VisMode::Spectrum.next(), VisMode::Scope);
        assert_eq!(VisMode::Scope.next(), VisMode::Off);
        assert_eq!(VisMode::Off.next(), VisMode::Spectrum);
    }

    #[test]
    fn bars_sit_four_skin_pixels_apart() {
        assert_eq!(bar_x(24, 0), 24);
        assert_eq!(bar_x(24, 1), 28);
        assert_eq!(bar_x(24, 18), 24 + 4 * 18);
    }

    #[test]
    fn bar_row_color_starts_at_the_third_palette_entry() {
        assert_eq!(bar_row_color(0), 2);
        assert_eq!(bar_row_color(15), 17);
    }

    #[test]
    fn scope_color_starts_at_the_nineteenth_palette_entry() {
        assert_eq!(scope_color(7), 18); // the centre row, brightest shade
        assert_eq!(scope_color(0), 18 + 3);
    }

    #[test]
    fn scope_span_connects_a_row_to_the_one_before_it() {
        assert_eq!(scope_span(5, 7), (5, 7));
        assert_eq!(scope_span(7, 5), (6, 7));
        assert_eq!(scope_span(7, 7), (7, 7));
    }

    #[test]
    fn a_shells_marquee_restarts_from_the_top_when_the_text_changes() {
        let mut shell = WinampShell::new();
        let long = "Radiohead - Everything In Its Right Place (4:11)";
        let first = shell.marquee(long, 0.0);
        assert_eq!(first.chars().count(), MARQUEE_CHARS);
        assert!(long.starts_with(&first));
        // Not yet a full step.
        assert_eq!(shell.marquee(long, 0.1), first);
        let stepped = shell.marquee(long, 0.22);
        assert!(long[1..].starts_with(&stepped));
        // A new title starts over from its own beginning.
        let other = "Someone Else - A Different Song Entirely (3:00)";
        assert!(other.starts_with(&shell.marquee(other, 0.5)));
    }
}
