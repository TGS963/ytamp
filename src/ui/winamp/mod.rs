//! Ported from fastpotify (MIT, Copyright (c) 2026 Carmine Paolino), src/ui/winamp/mod.rs
//! and src/winamp.rs.
//!
//! The Winamp skin window: the main window's controls, drawn through
//! the skin the listener has on. `App` owns the [`WinampShell`] and
//! opens the window as an egui viewport; this module reads `&State`
//! and the shell, draws, and returns the actions the listener asked
//! for, the same shape as every other view in `ui`.

mod equalizer;
mod pixel_text;
mod playlist;
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

impl WinampShell {
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts a skin on. Its textures are dropped, so the next frame
    /// rebuilds them from the new bitmaps. `App::reduce` calls this
    /// directly on `Action::SkinLoaded(Ok(_))`, since the decoded
    /// skin never passes through `State`.
    pub fn wear(&mut self, skin: Arc<Skin>) {
        self.skin = skin;
        self.textures.clear();
    }

    /// The skin's sheets as textures for `ctx`, made now if they are
    /// not yet, and remade if `ctx` is a different viewport than last
    /// time.
    fn textures(&mut self, ctx: &egui::Context) -> HashMap<Sheet, egui::TextureId> {
        if self.texture_ctx.as_ref() != Some(ctx) {
            self.textures.clear();
            // The playlist's own textures belong to the same context.
            self.playlist_text.clear();
            self.texture_ctx = Some(ctx.clone());
        }
        for sheet in Sheet::ALL {
            if self.textures.contains_key(&sheet) {
                continue;
            }
            let bitmap = self.skin.sheet(sheet);
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [bitmap.width as usize, bitmap.height as usize],
                &bitmap.rgba,
            );
            let handle = ctx.load_texture(
                format!("winamp-{}", sheet.file_stem()),
                image,
                egui::TextureOptions::NEAREST,
            );
            self.textures.insert(sheet, handle);
        }
        self.textures
            .iter()
            .map(|(sheet, handle)| (*sheet, handle.id()))
            .collect()
    }

    /// The marquee's window onto `text` at `now`: the text itself
    /// when it fits, otherwise 31 characters that step one character
    /// at a time. A text change restarts the scroll from the top.
    fn marquee(&mut self, text: &str, now: f64) -> String {
        if text != self.marquee_text {
            self.marquee_text = text.to_string();
            self.marquee_cursor = 0;
            self.marquee_last_step = now;
        }
        if !marquee_scrolls(&self.marquee_text) {
            return self.marquee_text.clone();
        }
        let steps = marquee_steps_since(now, self.marquee_last_step);
        self.marquee_cursor = self.marquee_cursor.wrapping_add(steps);
        self.marquee_last_step += steps as f64 * MARQUEE_STEP_SECS;
        marquee_window(&self.marquee_text, self.marquee_cursor)
    }

    /// The notice to show in the marquee, if a new one has arrived or
    /// one shown recently is still within its few seconds on screen.
    fn current_notice(&mut self, notices: &[String], now: f64) -> Option<String> {
        let latest = notices.last().map(String::as_str);
        match (&self.shown_notice, latest) {
            (Some((shown, _)), Some(latest)) if shown == latest => {}
            (_, Some(latest)) => self.shown_notice = Some((latest.to_string(), now)),
            (_, None) => {}
        }
        let (text, started) = self.shown_notice.as_ref()?;
        (now - started < NOTICE_DURATION_SECS).then(|| text.clone())
    }

    /// The whole stack's height in skin pixels: the main window, plus
    /// the playlist window under it while it is open.
    pub fn stack_height(&self) -> u32 {
        stack_height(
            self.shade,
            self.playlist_open,
            self.playlist_shade,
            self.playlist_height,
        ) + if self.equalizer_open {
            if self.equalizer_shade {
                layout::EQ_SHADE_HEIGHT
            } else {
                layout::EQ_HEIGHT
            }
        } else {
            0
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
        shade_bar(&mut view, &ctx, state, shell, out, focused);
        false
    } else {
        full_window(&mut view, &ctx, state, shell, out, focused, time)
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

/// The main window as it usually looks: background, title bar, the
/// readouts, the sliders, and the transport. Returns whether the
/// visualiser is still moving.
fn full_window(
    view: &mut View,
    ctx: &egui::Context,
    state: &State,
    shell: &mut WinampShell,
    out: &mut Vec<Action>,
    focused: bool,
    time: f64,
) -> bool {
    view.sprite(
        sprites::MAIN_BACKGROUND,
        Area::new(0, 0, layout::WINDOW_WIDTH, layout::WINDOW_HEIGHT),
    );
    title_bar(view, ctx, state, shell, out, focused);
    status_indicator(view, state);
    channel_lamps(view, state);
    time_display(view, state, shell, time);
    rates(view, state);
    let volume_event = volume_slider(view, state, out);
    balance_slider(view, state, out);
    let position_event = position_slider(view, state, out);
    marquee(view, state, shell, time, volume_event, position_event);
    let vis_moving = visualiser(view, state, shell);
    windows_buttons(view, shell);
    play_pause_stop(view, state, out);
    simple_transport(view, out);
    shuffle_repeat(view, state, out);
    clutter_bar(view, state, out);
    vis_moving
}

/// The window rolled up to its title bar: the time, a little seek
/// bar, and six unlabelled buttons that only listen, painted into the
/// bar's own bitmap.
fn shade_bar(
    view: &mut View,
    ctx: &egui::Context,
    state: &State,
    shell: &mut WinampShell,
    out: &mut Vec<Action>,
    focused: bool,
) {
    let bar = if focused {
        sprites::SHADE_BAR_ACTIVE
    } else {
        sprites::SHADE_BAR_INACTIVE
    };
    view.sprite(
        bar,
        Area::new(0, 0, layout::WINDOW_WIDTH, layout::SHADE_HEIGHT),
    );
    drag_and_shade(view, ctx, shell, "shade-bar", layout::TITLE_BAR);
    if close_button(view).clicked() {
        out.push(Action::WinampToggled);
    }
    if shade_button(view, shell.shade).clicked() {
        shell.shade = !shell.shade;
    }
    let whole = Area::new(
        layout::SHADE_TIME.x,
        layout::SHADE_TIME.y,
        layout::SHADE_TIME.width,
        layout::SHADE_TIME.height,
    );
    if view.interact(whole, "shade-time", Sense::click()).clicked() {
        shell.time_remaining = !shell.time_remaining;
    }
    let shown = shown_time(state, shell);
    view.text(&shade_time_text(shown, shell.time_remaining), whole);
    mini_transport(view, state, out);
    shade_position(view, state, out);
}

/// Reads and reacts to the title bar's drag, double-click, right-click
/// menu, and close and shade buttons.
fn title_bar(
    view: &mut View,
    ctx: &egui::Context,
    state: &State,
    shell: &mut WinampShell,
    out: &mut Vec<Action>,
    focused: bool,
) {
    let bar = if focused {
        sprites::TITLE_BAR_ACTIVE
    } else {
        sprites::TITLE_BAR_INACTIVE
    };
    view.sprite(bar, layout::TITLE_BAR);
    let title = drag_and_shade(view, ctx, shell, "title", layout::TITLE_BAR);
    let unit = view.unit;
    options_menu(
        egui::Popup::context_menu(&title),
        view.skin,
        state,
        unit,
        out,
    );
    if close_button(view).clicked() {
        out.push(Action::WinampToggled);
    }
    if shade_button(view, shell.shade).clicked() {
        shell.shade = !shell.shade;
    }
}

/// The title bar's own drag-to-move and double-click-to-shade
/// behaviour, shared with the shade bar. Returns the area's response,
/// so a caller can hang a right-click menu off it.
fn drag_and_shade(
    view: &mut View,
    ctx: &egui::Context,
    shell: &mut WinampShell,
    id: &str,
    area: Area,
) -> egui::Response {
    let response = view.interact(area, id, Sense::click_and_drag());
    if response.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if response.double_clicked() {
        shell.shade = !shell.shade;
    }
    response
}

fn close_button(view: &mut View) -> egui::Response {
    view.button(
        layout::CLOSE_BUTTON,
        sprites::CLOSE_BUTTON,
        sprites::CLOSE_BUTTON_PRESSED,
        "close",
    )
}

fn shade_button(view: &mut View, shaded: bool) -> egui::Response {
    let (normal, pressed) = if shaded {
        (sprites::UNSHADE_BUTTON, sprites::UNSHADE_BUTTON_PRESSED)
    } else {
        (sprites::SHADE_BUTTON, sprites::SHADE_BUTTON_PRESSED)
    };
    view.button(layout::SHADE_BUTTON, normal, pressed, "shade")
}

/// The play, pause, and stop lamp.
fn status_indicator(view: &mut View, state: &State) {
    let sprite = match state.playback.status {
        PlayStatus::Playing | PlayStatus::Loading => sprites::STATUS_PLAYING,
        PlayStatus::Paused => sprites::STATUS_PAUSED,
        PlayStatus::Stopped => sprites::STATUS_STOPPED,
    };
    view.sprite(sprite, layout::STATUS);
}

/// The mono and stereo lamps, lit by the decoded stream's channel
/// count.
fn channel_lamps(view: &mut View, state: &State) {
    let channels = state.playback.channels;
    let mono = if channels == 1 {
        sprites::MONO_ON
    } else {
        sprites::MONO_OFF
    };
    let stereo = if channels >= 2 {
        sprites::STEREO_ON
    } else {
        sprites::STEREO_OFF
    };
    view.sprite(mono, layout::MONO);
    view.sprite(stereo, layout::STEREO);
}

/// The digit cells' whole span, for the click that toggles counting
/// down instead of up.
fn time_display_area() -> Area {
    Area::new(
        layout::MINUS_EX.x,
        layout::MINUS_EX.y,
        layout::SECOND_ONES.x + layout::SECOND_ONES.width - layout::MINUS_EX.x,
        layout::MINUS_EX.height,
    )
}

/// The elapsed or remaining time, in the skin's digits, blinking at
/// 2 Hz while paused. A click on the digits swaps counting up for
/// counting down.
fn time_display(view: &mut View, state: &State, shell: &mut WinampShell, time: f64) {
    if view
        .interact(time_display_area(), "time", Sense::click())
        .clicked()
    {
        shell.time_remaining = !shell.time_remaining;
    }
    let extended = view.skin.has_extended_digits();
    if state.playback.status == PlayStatus::Stopped {
        blank_digits(view, extended);
        return;
    }
    if state.playback.status == PlayStatus::Paused && blinked_off(time) {
        blank_digits(view, extended);
        return;
    }
    let position = state.playback.position;
    let duration = state.playback.track_duration.unwrap_or(Duration::ZERO);
    let remaining = shell.time_remaining && duration > Duration::ZERO;
    let shown = shown_duration(position, duration, remaining);
    let digits = time_digits(shown);
    for (value, cell) in digits.into_iter().zip(layout::TIME_DIGITS) {
        let sprite = if extended {
            sprites::digit_ex(value)
        } else {
            sprites::digit(value)
        };
        view.sprite(sprite, cell);
    }
    let minus = match (extended, remaining) {
        (true, true) => sprites::NUMS_EX_MINUS,
        (true, false) => sprites::NUMS_EX_BLANK,
        (false, true) => sprites::NUMBERS_MINUS,
        (false, false) => sprites::NUMBERS_NO_MINUS,
    };
    view.sprite(minus, layout::MINUS_EX);
}

/// Whether this half-second is the blink's off half.
fn blinked_off(time: f64) -> bool {
    (time * 2.0).floor() as i64 % 2 == 1
}

fn blank_digits(view: &mut View, extended: bool) {
    if extended {
        for cell in layout::TIME_DIGITS {
            view.sprite(sprites::NUMS_EX_BLANK, cell);
        }
        view.sprite(sprites::NUMS_EX_BLANK, layout::MINUS_EX);
    } else {
        for cell in layout::TIME_DIGITS {
            view.sprite(sprites::NUMBERS_BLANK, cell);
        }
        view.sprite(sprites::NUMBERS_NO_MINUS, layout::MINUS);
    }
}

/// The time to show: the position, or the time left, with the minus
/// sign a caller draws separately.
pub fn shown_duration(position: Duration, duration: Duration, remaining: bool) -> Duration {
    if remaining {
        duration.saturating_sub(position)
    } else {
        position
    }
}

/// A duration as the four digits the time display's cells hold:
/// minutes tens, minutes ones, seconds tens, seconds ones. Minutes cap
/// at 99, as Winamp's two digits did.
pub fn time_digits(shown: Duration) -> [u32; 4] {
    let total_seconds = shown.as_secs();
    let minutes = (total_seconds / 60).min(99);
    let seconds = total_seconds % 60;
    [
        (minutes / 10) as u32,
        (minutes % 10) as u32,
        (seconds / 10) as u32,
        (seconds % 10) as u32,
    ]
}

/// The shade bar's tiny time text: `" 3:07"` or `"-1:02"`.
fn shade_time_text(shown: Duration, remaining: bool) -> String {
    format!(
        "{}{}",
        if remaining { "-" } else { " " },
        format_minutes_seconds(shown)
    )
}

pub(super) fn format_minutes_seconds(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", (seconds / 60).min(99), seconds % 60)
}

fn shown_time(state: &State, shell: &WinampShell) -> Duration {
    let duration = state.playback.track_duration.unwrap_or(Duration::ZERO);
    let remaining = shell.time_remaining && duration > Duration::ZERO;
    shown_duration(state.playback.position, duration, remaining)
}

/// "artist - title (m:ss)", or the app's name with nothing playing.
fn track_marquee_text(state: &State) -> String {
    let Some(track) = state.playback.queue.current() else {
        return "ytamp".to_string();
    };
    let name = if track.artists.is_empty() {
        track.title.clone()
    } else {
        format!("{} - {}", track.artist_names(), track.title)
    };
    match state.playback.track_duration {
        Some(duration) if !duration.is_zero() => {
            format!("{name} ({})", format_minutes_seconds(duration))
        }
        _ => name,
    }
}

/// The value a slider drag or click reported this frame, if any.
fn slider_active_value(event: SliderEvent) -> Option<f32> {
    match event {
        SliderEvent::Dragging(value) | SliderEvent::Committed(value) => Some(value),
        SliderEvent::None => None,
    }
}

/// What the marquee says: `VOLUME: NN%` while the volume drags,
/// `SEEK TO: m:ss/m:ss` while the seek bar drags, a fresh notice once,
/// else the track line.
fn marquee_priority_text(
    track_line: &str,
    volume: Option<f32>,
    seek: Option<(Duration, Duration)>,
    notice: Option<&str>,
) -> String {
    if let Some(volume) = volume {
        return format!("VOLUME: {}%", (volume * 100.0).round() as u32);
    }
    if let Some((target, duration)) = seek {
        return format!(
            "SEEK TO: {}/{}",
            format_minutes_seconds(target),
            format_minutes_seconds(duration)
        );
    }
    if let Some(notice) = notice {
        return notice.to_string();
    }
    track_line.to_string()
}

/// Draws the marquee: works out what it should say this frame, steps
/// its scroll, and asks for another frame soon if it is still moving.
fn marquee(
    view: &mut View,
    state: &State,
    shell: &mut WinampShell,
    time: f64,
    volume_event: SliderEvent,
    position_event: SliderEvent,
) {
    let duration = state.playback.track_duration.unwrap_or(Duration::ZERO);
    let seek = slider_active_value(position_event)
        .filter(|_| !duration.is_zero())
        .map(|fraction| (duration.mul_f32(fraction), duration));
    let notice = state
        .playback
        .error
        .clone()
        .or_else(|| shell.current_notice(&state.notices, time));
    let track_line = track_marquee_text(state);
    let text = marquee_priority_text(
        &track_line,
        slider_active_value(volume_event),
        seek,
        notice.as_deref(),
    );
    let shown = shell.marquee(&text, time);
    if marquee_scrolls(&text) {
        view.ui.ctx().request_repaint_after(MARQUEE_REPAINT);
    }
    view.text(&shown, layout::MARQUEE);
}

/// The bitrate (a stand-in, since ytamp streams at whatever YouTube
/// sent) and the decoder's sample rate in kHz.
fn rates(view: &mut View, state: &State) {
    if state.playback.status == PlayStatus::Stopped {
        return;
    }
    view.text("128", layout::KBPS);
    if state.playback.sample_rate > 0 {
        view.text(
            &(state.playback.sample_rate / 1000).to_string(),
            layout::KHZ,
        );
    }
}

/// Where a fraction along a slider's travel sits, as a fraction from
/// 0 to 1: the value while a drag holds it, else the value that fits
/// the current position.
fn slider_fraction(event: SliderEvent, resting: f32) -> f32 {
    match event {
        SliderEvent::Dragging(value) | SliderEvent::Committed(value) => value,
        SliderEvent::None => resting,
    }
}

fn volume_slider(view: &mut View, state: &State, out: &mut Vec<Action>) -> SliderEvent {
    let (response, event) = view.slider(layout::VOLUME, "volume", 14);
    if let SliderEvent::Dragging(value) | SliderEvent::Committed(value) = event {
        out.push(Action::VolumeSet(value));
    }
    let fraction = slider_fraction(event, state.playback.volume);
    let frame = (fraction * (sprites::SLIDER_FRAMES - 1) as f32).round() as u32;
    view.sprite(sprites::volume_frame(frame), layout::VOLUME);
    let thumb = if response.dragged() || response.is_pointer_button_down_on() {
        sprites::VOLUME_THUMB_PRESSED
    } else {
        sprites::VOLUME_THUMB
    };
    let thumb_x = layout::VOLUME.x + (fraction * layout::VOLUME_TRAVEL as f32).round() as u32;
    view.sprite_at(thumb, thumb_x, layout::VOLUME.y + 1);
    event
}

/// Stereo balance, independent of EQ bypass.
fn balance_slider(view: &mut View, state: &State, out: &mut Vec<Action>) {
    let (response, event) = view.slider(layout::BALANCE, "balance", 14);
    let mut fraction = slider_fraction(event, (state.playback.balance + 1.) / 2.);
    if response.double_clicked() {
        fraction = 0.5;
        out.push(Action::BalanceSet(0.));
    } else if let Some(value) = slider_active_value(event) {
        out.push(Action::BalanceSet(value * 2. - 1.));
    }
    let value = fraction * 2. - 1.;
    view.sprite(
        sprites::balance_frame((value.abs() * 27.).round() as u32),
        layout::BALANCE,
    );
    let thumb = if response.dragged() || response.is_pointer_button_down_on() {
        sprites::BALANCE_THUMB_PRESSED
    } else {
        sprites::BALANCE_THUMB
    };
    view.sprite_at(
        thumb,
        layout::BALANCE.x + (fraction * layout::BALANCE_TRAVEL as f32).round() as u32,
        layout::BALANCE.y + 1,
    );
    response.on_hover_text(crate::ui::equalizer::balance_label(value));
}

fn position_slider(view: &mut View, state: &State, out: &mut Vec<Action>) -> SliderEvent {
    view.sprite(sprites::POSITION_TRACK, layout::POSITION);
    let duration = state.playback.track_duration.unwrap_or(Duration::ZERO);
    if duration.is_zero() || state.playback.status == PlayStatus::Stopped {
        return SliderEvent::None;
    }
    let (response, event) = view.slider(layout::POSITION, "position", 29);
    if let SliderEvent::Committed(value) = event {
        out.push(Action::SeekRequested(duration.mul_f32(value)));
    }
    let resting = seek_fraction(state.playback.position, duration);
    let fraction = slider_fraction(event, resting);
    let thumb = if response.dragged() || response.is_pointer_button_down_on() {
        sprites::POSITION_THUMB_PRESSED
    } else {
        sprites::POSITION_THUMB
    };
    let thumb_x = layout::POSITION.x + (fraction * layout::POSITION_TRAVEL as f32).round() as u32;
    view.sprite_at(thumb, thumb_x, layout::POSITION.y);
    event
}

/// The seek bar's fraction along its travel: 0 at the start of the
/// track, 1 at the end.
pub fn seek_fraction(position: Duration, duration: Duration) -> f32 {
    if duration.is_zero() {
        0.0
    } else {
        (position.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
    }
}

/// The shade bar's small seek bar: the same fraction as the main
/// window's, drawn along a shorter track.
fn shade_position(view: &mut View, state: &State, out: &mut Vec<Action>) {
    view.sprite(sprites::SHADE_POSITION_TRACK, layout::SHADE_POSITION);
    let duration = state.playback.track_duration.unwrap_or(Duration::ZERO);
    if duration.is_zero() || state.playback.status == PlayStatus::Stopped {
        return;
    }
    let (response, event) = view.slider(layout::SHADE_POSITION, "shade-position", 3);
    if let SliderEvent::Committed(value) = event {
        out.push(Action::SeekRequested(duration.mul_f32(value)));
    }
    let resting = seek_fraction(state.playback.position, duration);
    let fraction = slider_fraction(event, resting);
    let thumb = if response.dragged() {
        sprites::SHADE_POSITION_THUMB_RIGHT
    } else {
        sprites::SHADE_POSITION_THUMB
    };
    let travel = layout::SHADE_POSITION.width - 3;
    let thumb_x = layout::SHADE_POSITION.x + (fraction * travel as f32).round() as u32;
    view.sprite_at(thumb, thumb_x, layout::SHADE_POSITION.y);
}

/// The display's box: the spectrum bars, the oscilloscope, or
/// nothing, in the skin's own `viscolor.txt` colours. A click cycles
/// to the next mode. Returns whether anything is still moving, so the
/// caller can keep asking for frames while the bars fall.
fn visualiser(view: &mut View, state: &State, shell: &mut WinampShell) -> bool {
    let area = layout::VISUALIZER;
    if view
        .interact(area, "visualiser", Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        shell.vis_mode = shell.vis_mode.next();
    }
    if shell.vis_mode == VisMode::Off {
        return false;
    }
    let palette = view.skin.vis_colors;
    let color = move |index: usize| {
        let [r, g, b] = palette[index];
        Color32::from_rgb(r, g, b)
    };
    view.fill(area.x, area.y, area.width, area.height, color(0));
    for y in (0..area.height).step_by(2) {
        for x in (0..area.width).step_by(2) {
            view.fill(area.x + x, area.y + y, 1, 1, color(1));
        }
    }
    let sounding = matches!(
        state.playback.status,
        PlayStatus::Playing | PlayStatus::Loading
    );
    if shell.vis_mode == VisMode::Scope {
        draw_scope(view, area, sounding, color)
    } else {
        draw_spectrum(view, area, shell, sounding, color)
    }
}

/// The spectrum bars: read from the audio tap, stepped by the
/// analyser, and drawn with their peaks. Returns whether they are
/// still moving.
fn draw_spectrum(
    view: &mut View,
    area: Area,
    shell: &mut WinampShell,
    sounding: bool,
    color: impl Fn(usize) -> Color32,
) -> bool {
    let samples = if sounding {
        AudioTap::shared().window(vis::FFT_SAMPLES, vis::LAG)
    } else {
        vec![0.0; vis::FFT_SAMPLES]
    };
    let bars = shell.vis_analyser.step(&samples, Instant::now());
    for (index, bar) in bars.iter().enumerate() {
        let x = bar_x(area.x, index);
        for row in (vis::ROWS - bar.height)..vis::ROWS {
            view.fill(x, area.y + u32::from(row), 3, 1, color(bar_row_color(row)));
        }
        if let Some(peak) = bar.peak {
            let row = vis::ROWS - peak;
            view.fill(x, area.y + u32::from(row), 3, 1, color(PEAK_COLOR));
        }
    }
    sounding || !shell.vis_analyser.settled()
}

/// The oscilloscope trace: read from the audio tap, and shaded by how
/// far each row sits from the centre.
fn draw_scope(
    view: &mut View,
    area: Area,
    sounding: bool,
    color: impl Fn(usize) -> Color32,
) -> bool {
    let samples = if sounding {
        AudioTap::shared().window(vis::SCOPE_SAMPLES, vis::LAG)
    } else {
        vec![0.0; vis::SCOPE_SAMPLES]
    };
    let rows = vis::scope(&samples);
    let mut last = rows[0];
    for (x, &y) in rows.iter().enumerate() {
        let (top, bottom) = scope_span(y, last);
        last = y;
        let shade = color(scope_color(y));
        for row in top..=bottom {
            view.fill(area.x + x as u32, area.y + u32::from(row), 1, 1, shade);
        }
    }
    sounding
}

/// The palette index for the analyser's peak mark, past the sixteen
/// bar-row colours.
const PEAK_COLOR: usize = 23;

/// The x position, in skin pixels, of the bar at `index`: each bar is
/// three columns wide with one gap.
fn bar_x(area_x: u32, index: usize) -> u32 {
    area_x + 4 * index as u32
}

/// The palette index for a bar's row, counting up from the bar-row
/// colours at `viscolor[2..18]`.
fn bar_row_color(row: u8) -> usize {
    2 + usize::from(row)
}

/// The palette index for a scope row, among the five oscilloscope
/// colours at `viscolor[18..23]`.
fn scope_color(row: u8) -> usize {
    18 + vis::scope_shade(row)
}

/// The rows a scope column fills between its own row and the row
/// before it, so the trace draws as a connected line rather than
/// separate dots.
fn scope_span(row: u8, previous: u8) -> (u8, u8) {
    if previous < row {
        (previous + 1, row)
    } else {
        (row, previous)
    }
}

/// Open and close the attached equalizer and playlist panels.
fn windows_buttons(view: &mut View, shell: &mut WinampShell) {
    let (normal, pressed) = if shell.equalizer_open {
        (sprites::EQ_ON, sprites::EQ_ON_PRESSED)
    } else {
        (sprites::EQ_OFF, sprites::EQ_OFF_PRESSED)
    };
    if view
        .button(layout::EQ_BUTTON, normal, pressed, "equalizer")
        .on_hover_text("Equalizer")
        .clicked()
    {
        shell.equalizer_open = !shell.equalizer_open;
    }
    let (normal, pressed) = if shell.playlist_open {
        (sprites::PLAYLIST_ON, sprites::PLAYLIST_ON_PRESSED)
    } else {
        (sprites::PLAYLIST_OFF, sprites::PLAYLIST_OFF_PRESSED)
    };
    if view
        .button(layout::PLAYLIST_BUTTON, normal, pressed, "playlist")
        .clicked()
    {
        shell.playlist_open = !shell.playlist_open;
    }
}

/// What a click on Play does: starts the queue, unless it is already
/// playing.
fn play_click(playing: bool) -> Option<Action> {
    (!playing).then_some(Action::PlayToggled)
}

/// What a click on Pause does: pauses the queue, only while it plays.
fn pause_click(playing: bool) -> Option<Action> {
    playing.then_some(Action::PlayToggled)
}

/// What a click on Stop does: pauses if the queue was playing, and
/// always rewinds to the start.
fn stop_click(playing: bool) -> [Option<Action>; 2] {
    [
        playing.then_some(Action::PlayToggled),
        Some(Action::SeekRequested(Duration::ZERO)),
    ]
}

/// Play, pause, and stop: the three buttons whose meaning depends on
/// whether the queue is already playing. The buttons never latch, as
/// in Winamp. Only a held pointer shows the pressed sprite, and the
/// lamp next to the time shows the state.
fn play_pause_stop(view: &mut View, state: &State, out: &mut Vec<Action>) {
    let playing = state.playback.status == PlayStatus::Playing;
    push_if_clicked(
        view.button(layout::PLAY, sprites::PLAY, sprites::PLAY_PRESSED, "play"),
        out,
        play_click(playing),
    );
    push_if_clicked(
        view.button(
            layout::PAUSE,
            sprites::PAUSE,
            sprites::PAUSE_PRESSED,
            "pause",
        ),
        out,
        pause_click(playing),
    );
    let stop = view.button(layout::STOP, sprites::STOP, sprites::STOP_PRESSED, "stop");
    if stop.clicked() {
        out.extend(stop_click(playing).into_iter().flatten());
    }
}

/// Adds `action` to `out` when `response` was clicked this frame.
fn push_if_clicked(response: egui::Response, out: &mut Vec<Action>, action: Option<Action>) {
    if response.clicked() {
        out.extend(action);
    }
}

/// Previous, next, and eject: buttons whose meaning never depends on
/// the current status.
fn simple_transport(view: &mut View, out: &mut Vec<Action>) {
    if view
        .button(
            layout::PREVIOUS,
            sprites::PREVIOUS,
            sprites::PREVIOUS_PRESSED,
            "previous",
        )
        .clicked()
    {
        out.push(Action::PreviousPressed);
    }
    if view
        .button(layout::NEXT, sprites::NEXT, sprites::NEXT_PRESSED, "next")
        .clicked()
    {
        out.push(Action::NextPressed);
    }
    if view
        .button(
            layout::EJECT,
            sprites::EJECT,
            sprites::EJECT_PRESSED,
            "eject",
        )
        .clicked()
    {
        out.push(Action::WinampToggled);
    }
}

/// A shade-bar transport button: no bitmap of its own, since the
/// bar's background already draws it; this only listens for a click.
fn mini_button(
    view: &mut View,
    id: &str,
    area: Area,
    out: &mut Vec<Action>,
    action: Option<Action>,
) {
    let response = view.interact(area, id, Sense::click());
    push_if_clicked(response, out, action);
}

/// The shade bar's six small transport buttons.
fn mini_transport(view: &mut View, state: &State, out: &mut Vec<Action>) {
    let playing = state.playback.status == PlayStatus::Playing;
    mini_button(
        view,
        "shade-previous",
        layout::SHADE_PREVIOUS,
        out,
        Some(Action::PreviousPressed),
    );
    mini_button(
        view,
        "shade-play",
        layout::SHADE_PLAY,
        out,
        play_click(playing),
    );
    mini_button(
        view,
        "shade-pause",
        layout::SHADE_PAUSE,
        out,
        pause_click(playing),
    );
    let stop = view.interact(layout::SHADE_STOP, "shade-stop", Sense::click());
    if stop.clicked() {
        out.extend(stop_click(playing).into_iter().flatten());
    }
    mini_button(
        view,
        "shade-next",
        layout::SHADE_NEXT,
        out,
        Some(Action::NextPressed),
    );
    mini_button(
        view,
        "shade-eject",
        layout::SHADE_EJECT,
        out,
        Some(Action::WinampToggled),
    );
}

fn shuffle_repeat(view: &mut View, state: &State, out: &mut Vec<Action>) {
    let shuffle_on = state.playback.queue.shuffle;
    let (normal, pressed) = if shuffle_on {
        (sprites::SHUFFLE_ON, sprites::SHUFFLE_ON_PRESSED)
    } else {
        (sprites::SHUFFLE_OFF, sprites::SHUFFLE_OFF_PRESSED)
    };
    if view
        .button(layout::SHUFFLE, normal, pressed, "shuffle")
        .clicked()
    {
        out.push(Action::ShuffleToggled);
    }
    let repeat_on = state.playback.queue.repeat != RepeatMode::Off;
    let (normal, pressed) = if repeat_on {
        (sprites::REPEAT_ON, sprites::REPEAT_ON_PRESSED)
    } else {
        (sprites::REPEAT_OFF, sprites::REPEAT_OFF_PRESSED)
    };
    if view
        .button(layout::REPEAT, normal, pressed, "repeat")
        .clicked()
    {
        out.push(Action::RepeatCycled);
    }
}

/// The O button in the main window's clutter strip.
/// Lights while held, and opens the same menu a right-click on the
/// title bar does.
fn clutter_bar(view: &mut View, state: &State, out: &mut Vec<Action>) {
    view.sprite(sprites::CLUTTER_BAR, layout::CLUTTER_BAR);
    let options = view
        .interact(layout::CLUTTER_O, "clutter-o", Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if options.is_pointer_button_down_on() {
        view.sprite(sprites::CLUTTER_O_LIT, layout::CLUTTER_O);
    }
    let unit = view.unit;
    options_menu(egui::Popup::menu(&options), view.skin, state, unit, out);
}

/// The menu behind a right-click on the title bar and the O button:
/// the window's scale, always-on-top, the skin picker, and the way
/// back to the classic look.
fn options_menu(
    popup: egui::Popup<'_>,
    skin: &Skin,
    state: &State,
    unit: f32,
    out: &mut Vec<Action>,
) {
    menu(popup, skin, unit, |ui| {
        ui.set_min_width(menu_font(unit) * 11.0);
        if let Some(action) = scale_menu_row(ui, state.winamp.scale) {
            out.push(action);
        }
        let mut on_top = state.winamp.on_top;
        if ui.checkbox(&mut on_top, "Always on top").clicked() {
            out.push(Action::WinampOnTopToggled);
        }
        ui.separator();
        if let Some(action) = skin_menu_rows(ui, &state.winamp.skin, &state.winamp.available_skins)
        {
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
        if let Some(error) = &state.playback.error {
            ui.label(error);
            if ui.button("Retry playback").clicked() {
                out.push(Action::PlaybackRetryRequested);
                ui.close();
            }
            ui.separator();
        }
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
    });
}

/// The 1x through 4x radio row.
fn scale_menu_row(ui: &mut Ui, current: u8) -> Option<Action> {
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
fn skin_menu_rows(ui: &mut Ui, current: &Option<String>, names: &[String]) -> Option<Action> {
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
pub(super) fn menu<R>(
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
    let font = menu_font(unit);
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
            .max_height(menu_limit(ui))
            .show(ui, contents)
            .inner
    })
}

/// The type size of a menu at this scale.
fn menu_font(unit: f32) -> f32 {
    (5.0 * unit).clamp(9.0, 14.0)
}

/// How tall a menu's contents may be before they scroll: the window
/// less the menu's own frame, so egui can always find it a place
/// inside.
fn menu_limit(ui: &Ui) -> f32 {
    let frame = ui.spacing().menu_margin.sum().y + 6.0;
    (ui.ctx().content_rect().height() - frame).max(menu_font(1.0))
}

#[cfg(test)]
mod tests {
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
