use super::*;

/// The main window as it usually looks: background, title bar, the
/// readouts, the sliders, and the transport. Returns whether the
/// visualiser is still moving.
pub(super) fn full_window(
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
    transport::windows_buttons(view, shell);
    transport::play_pause_stop(view, state, out);
    transport::simple_transport(view, out);
    transport::shuffle_repeat(view, state, out);
    transport::clutter_bar(view, state, out);
    vis_moving
}

/// The window rolled up to its title bar: the time, a little seek
/// bar, and six unlabelled buttons that only listen, painted into the
/// bar's own bitmap.
pub(super) fn shade_bar(
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
    transport::mini_transport(view, state, out);
    shade_position(view, state, out);
}

/// Reads and reacts to the title bar's drag, double-click, right-click
/// menu, and close and shade buttons.
pub(super) fn title_bar(
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
    super::menus::options_menu(
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
pub(super) fn drag_and_shade(
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

pub(super) fn close_button(view: &mut View) -> egui::Response {
    view.button(
        layout::CLOSE_BUTTON,
        sprites::CLOSE_BUTTON,
        sprites::CLOSE_BUTTON_PRESSED,
        "close",
    )
}

pub(super) fn shade_button(view: &mut View, shaded: bool) -> egui::Response {
    let (normal, pressed) = if shaded {
        (sprites::UNSHADE_BUTTON, sprites::UNSHADE_BUTTON_PRESSED)
    } else {
        (sprites::SHADE_BUTTON, sprites::SHADE_BUTTON_PRESSED)
    };
    view.button(layout::SHADE_BUTTON, normal, pressed, "shade")
}

/// The play, pause, and stop lamp.
pub(super) fn status_indicator(view: &mut View, state: &State) {
    let sprite = match state.playback.status {
        PlayStatus::Playing | PlayStatus::Loading => sprites::STATUS_PLAYING,
        PlayStatus::Paused => sprites::STATUS_PAUSED,
        PlayStatus::Stopped => sprites::STATUS_STOPPED,
    };
    view.sprite(sprite, layout::STATUS);
}

/// The mono and stereo lamps, lit by the decoded stream's channel
/// count.
pub(super) fn channel_lamps(view: &mut View, state: &State) {
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
pub(super) fn time_display_area() -> Area {
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
pub(super) fn time_display(view: &mut View, state: &State, shell: &mut WinampShell, time: f64) {
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
pub(super) fn blinked_off(time: f64) -> bool {
    (time * 2.0).floor() as i64 % 2 == 1
}

pub(super) fn blank_digits(view: &mut View, extended: bool) {
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
pub(super) fn shade_time_text(shown: Duration, remaining: bool) -> String {
    format!(
        "{}{}",
        if remaining { "-" } else { " " },
        format_minutes_seconds(shown)
    )
}

pub(crate) fn format_minutes_seconds(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", (seconds / 60).min(99), seconds % 60)
}

pub(super) fn shown_time(state: &State, shell: &WinampShell) -> Duration {
    let duration = state.playback.track_duration.unwrap_or(Duration::ZERO);
    let remaining = shell.time_remaining && duration > Duration::ZERO;
    shown_duration(state.playback.position, duration, remaining)
}

/// "artist - title (m:ss)", or the app's name with nothing playing.
pub(super) fn track_marquee_text(state: &State) -> String {
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
pub(crate) fn slider_active_value(event: SliderEvent) -> Option<f32> {
    match event {
        SliderEvent::Dragging(value) | SliderEvent::Committed(value) => Some(value),
        SliderEvent::None => None,
    }
}

/// What the marquee says: `VOLUME: NN%` while the volume drags,
/// `SEEK TO: m:ss/m:ss` while the seek bar drags, a fresh notice once,
/// else the track line.
pub(super) fn marquee_priority_text(
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
pub(super) fn marquee(
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
pub(super) fn rates(view: &mut View, state: &State) {
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
pub(crate) fn slider_fraction(event: SliderEvent, resting: f32) -> f32 {
    match event {
        SliderEvent::Dragging(value) | SliderEvent::Committed(value) => value,
        SliderEvent::None => resting,
    }
}

pub(super) fn volume_slider(view: &mut View, state: &State, out: &mut Vec<Action>) -> SliderEvent {
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
pub(super) fn balance_slider(view: &mut View, state: &State, out: &mut Vec<Action>) {
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

pub(super) fn position_slider(
    view: &mut View,
    state: &State,
    out: &mut Vec<Action>,
) -> SliderEvent {
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
pub(super) fn shade_position(view: &mut View, state: &State, out: &mut Vec<Action>) {
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
pub(super) fn visualiser(view: &mut View, state: &State, shell: &mut WinampShell) -> bool {
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
pub(super) fn draw_spectrum(
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
pub(super) fn draw_scope(
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
pub(super) fn bar_x(area_x: u32, index: usize) -> u32 {
    area_x + 4 * index as u32
}

/// The palette index for a bar's row, counting up from the bar-row
/// colours at `viscolor[2..18]`.
pub(super) fn bar_row_color(row: u8) -> usize {
    2 + usize::from(row)
}

/// The palette index for a scope row, among the five oscilloscope
/// colours at `viscolor[18..23]`.
pub(super) fn scope_color(row: u8) -> usize {
    18 + vis::scope_shade(row)
}

/// The rows a scope column fills between its own row and the row
/// before it, so the trace draws as a connected line rather than
/// separate dots.
pub(super) fn scope_span(row: u8, previous: u8) -> (u8, u8) {
    if previous < row {
        (previous + 1, row)
    } else {
        (row, previous)
    }
}
