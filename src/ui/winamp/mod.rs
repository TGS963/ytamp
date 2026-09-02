//! Ported from fastpotify (MIT, Copyright (c) 2026 Carmine Paolino), src/ui/winamp/mod.rs
//! and src/winamp.rs.
//!
//! The Winamp skin window: the main window's controls, drawn through
//! the skin the listener has on. `App` owns the [`WinampShell`] and
//! opens the window as an egui viewport; this module reads `&State`
//! and the shell, draws, and returns the actions the listener asked
//! for, the same shape as every other view in `ui`.

mod view;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use egui::{Sense, Ui, ViewportCommand};

use crate::core::action::Action;
use crate::core::queue::RepeatMode;
use crate::core::state::{PlayStatus, State};
use crate::skin::layout::{self, Area};
use crate::skin::{Sheet, Skin, sprites};

pub use view::{SliderEvent, View};

/// How often the window repaints on its own while a track plays, so
/// the position and the blink move without a pointer event to wake it.
const PLAYING_REPAINT: Duration = Duration::from_millis(250);

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
}

impl Default for WinampShell {
    fn default() -> Self {
        Self {
            skin: Skin::builtin(),
            textures: HashMap::new(),
            texture_ctx: None,
            time_remaining: false,
            shade: false,
        }
    }
}

impl WinampShell {
    pub fn new() -> Self {
        Self::default()
    }

    /// The skin's sheets as textures for `ctx`, made now if they are
    /// not yet, and remade if `ctx` is a different viewport than last
    /// time.
    fn textures(&mut self, ctx: &egui::Context) -> HashMap<Sheet, egui::TextureId> {
        if self.texture_ctx.as_ref() != Some(ctx) {
            self.textures.clear();
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

/// The window's size in logical points, for `show_viewport_immediate`.
pub fn window_size_points(shade: bool, scale: u8, pixels_per_point: f32) -> egui::Vec2 {
    let unit = unit(scale, pixels_per_point);
    egui::vec2(layout::WINDOW_WIDTH as f32, window_height(shade) as f32) * unit
}

/// Draws the whole window and reads its controls, adding any action a
/// click or a drag produced to `out`.
pub fn show(ui: &mut Ui, state: &State, shell: &mut WinampShell, out: &mut Vec<Action>) {
    let ctx = ui.ctx().clone();
    let unit = unit(state.winamp.scale, ctx.pixels_per_point());
    let origin = ui.max_rect().min;
    let focused = ctx
        .input(|input| input.viewport().focused)
        .unwrap_or(true);
    let time = ctx.input(|input| input.time);
    let textures = shell.textures(&ctx);
    let skin = shell.skin.clone();
    let mut view = View {
        ui,
        origin,
        unit,
        skin: &skin,
        textures: &textures,
    };
    if shell.shade {
        shade_bar(&mut view, &ctx, state, shell, out, focused);
    } else {
        full_window(&mut view, &ctx, state, shell, out, focused, time);
    }
    if state.playback.status == PlayStatus::Playing || state.playback.status == PlayStatus::Paused
    {
        ctx.request_repaint_after(PLAYING_REPAINT);
    }
}

/// The main window as it usually looks: background, title bar, the
/// readouts, the sliders, and the transport.
fn full_window(
    view: &mut View,
    ctx: &egui::Context,
    state: &State,
    shell: &mut WinampShell,
    out: &mut Vec<Action>,
    focused: bool,
    time: f64,
) {
    view.sprite(
        sprites::MAIN_BACKGROUND,
        Area::new(0, 0, layout::WINDOW_WIDTH, layout::WINDOW_HEIGHT),
    );
    title_bar(view, ctx, shell, out, focused);
    status_indicator(view, state);
    channel_lamps(view, state);
    time_display(view, state, shell, time);
    marquee(view, state);
    rates(view, state);
    volume_slider(view, state, out);
    balance_slider(view);
    position_slider(view, state, out);
    windows_buttons(view);
    play_pause_stop(view, state, out);
    simple_transport(view, out);
    shuffle_repeat(view, state, out);
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
    view.sprite(bar, Area::new(0, 0, layout::WINDOW_WIDTH, layout::SHADE_HEIGHT));
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
    if view
        .interact(whole, "shade-time", Sense::click())
        .clicked()
    {
        shell.time_remaining = !shell.time_remaining;
    }
    let shown = shown_time(state, shell);
    view.text(&shade_time_text(shown, shell.time_remaining), whole);
    mini_transport(view, state, out);
    shade_position(view, state, out);
}

/// Reads and reacts to the title bar's drag, double-click, and close
/// and shade buttons.
fn title_bar(
    view: &mut View,
    ctx: &egui::Context,
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
    drag_and_shade(view, ctx, shell, "title", layout::TITLE_BAR);
    if close_button(view).clicked() {
        out.push(Action::WinampToggled);
    }
    if shade_button(view, shell.shade).clicked() {
        shell.shade = !shell.shade;
    }
}

/// The title bar's own drag-to-move and double-click-to-shade
/// behaviour, shared with the shade bar.
fn drag_and_shade(view: &mut View, ctx: &egui::Context, shell: &mut WinampShell, id: &str, area: Area) {
    let response = view.interact(area, id, Sense::click_and_drag());
    if response.drag_started() {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if response.double_clicked() {
        shell.shade = !shell.shade;
    }
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

fn format_minutes_seconds(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", (seconds / 60).min(99), seconds % 60)
}

fn shown_time(state: &State, shell: &WinampShell) -> Duration {
    let duration = state.playback.track_duration.unwrap_or(Duration::ZERO);
    let remaining = shell.time_remaining && duration > Duration::ZERO;
    shown_duration(state.playback.position, duration, remaining)
}

/// "artist - title", or the app's name with nothing playing.
fn marquee(view: &mut View, state: &State) {
    let text = match state.playback.queue.current() {
        Some(track) if track.artists.is_empty() => track.title.clone(),
        Some(track) => format!("{} - {}", track.artist_names(), track.title),
        None => "ytamp".to_string(),
    };
    view.text(&text, layout::MARQUEE);
}

/// The bitrate (a stand-in, since ytamp streams at whatever YouTube
/// sent) and the decoder's sample rate in kHz.
fn rates(view: &mut View, state: &State) {
    if state.playback.status == PlayStatus::Stopped {
        return;
    }
    view.text("128", layout::KBPS);
    if state.playback.sample_rate > 0 {
        view.text(&(state.playback.sample_rate / 1000).to_string(), layout::KHZ);
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

fn volume_slider(view: &mut View, state: &State, out: &mut Vec<Action>) {
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
}

/// The balance slider, always centred: ytamp's engine has no balance
/// control, so this draws Winamp's neutral look and nothing more.
fn balance_slider(view: &mut View) {
    view.sprite(sprites::balance_frame(14), layout::BALANCE);
    let thumb_x = layout::BALANCE.x + layout::BALANCE_TRAVEL / 2;
    view.sprite_at(sprites::BALANCE_THUMB, thumb_x, layout::BALANCE.y + 1);
}

fn position_slider(view: &mut View, state: &State, out: &mut Vec<Action>) {
    view.sprite(sprites::POSITION_TRACK, layout::POSITION);
    let duration = state.playback.track_duration.unwrap_or(Duration::ZERO);
    if duration.is_zero() || state.playback.status == PlayStatus::Stopped {
        return;
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

/// The EQ and PL toggles. Neither has a function yet: the equalizer
/// and the playlist window are later parts of this plan.
fn windows_buttons(view: &mut View) {
    view.sprite(sprites::EQ_OFF, layout::EQ_BUTTON);
    view.sprite(sprites::PLAYLIST_OFF, layout::PLAYLIST_BUTTON);
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
/// whether the queue is already playing.
fn play_pause_stop(view: &mut View, state: &State, out: &mut Vec<Action>) {
    let playing = state.playback.status == PlayStatus::Playing;
    let stopped = state.playback.status == PlayStatus::Stopped;
    let play_sprite = if playing { sprites::PLAY_PRESSED } else { sprites::PLAY };
    let pause_sprite = if playing { sprites::PAUSE_PRESSED } else { sprites::PAUSE };
    let stop_sprite = if stopped { sprites::STOP_PRESSED } else { sprites::STOP };
    push_if_clicked(
        view.button(layout::PLAY, play_sprite, sprites::PLAY_PRESSED, "play"),
        out,
        play_click(playing),
    );
    push_if_clicked(
        view.button(layout::PAUSE, pause_sprite, sprites::PAUSE_PRESSED, "pause"),
        out,
        pause_click(playing),
    );
    let stop = view.button(layout::STOP, stop_sprite, sprites::STOP_PRESSED, "stop");
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
        .button(layout::EJECT, sprites::EJECT, sprites::EJECT_PRESSED, "eject")
        .clicked()
    {
        out.push(Action::WinampToggled);
    }
}

/// A shade-bar transport button: no bitmap of its own, since the
/// bar's background already draws it; this only listens for a click.
fn mini_button(view: &mut View, id: &str, area: Area, out: &mut Vec<Action>, action: Option<Action>) {
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
    mini_button(view, "shade-play", layout::SHADE_PLAY, out, play_click(playing));
    mini_button(view, "shade-pause", layout::SHADE_PAUSE, out, pause_click(playing));
    let stop = view.interact(layout::SHADE_STOP, "shade-stop", Sense::click());
    if stop.clicked() {
        out.extend(stop_click(playing).into_iter().flatten());
    }
    mini_button(view, "shade-next", layout::SHADE_NEXT, out, Some(Action::NextPressed));
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
    if view.button(layout::SHUFFLE, normal, pressed, "shuffle").clicked() {
        out.push(Action::ShuffleToggled);
    }
    let repeat_on = state.playback.queue.repeat != RepeatMode::Off;
    let (normal, pressed) = if repeat_on {
        (sprites::REPEAT_ON, sprites::REPEAT_ON_PRESSED)
    } else {
        (sprites::REPEAT_OFF, sprites::REPEAT_OFF_PRESSED)
    };
    if view.button(layout::REPEAT, normal, pressed, "repeat").clicked() {
        out.push(Action::RepeatCycled);
    }
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
        assert_eq!(
            window_size_points(false, 2, 1.0),
            egui::vec2(550.0, 232.0)
        );
        assert_eq!(window_size_points(true, 1, 1.0), egui::vec2(275.0, 14.0));
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
}
