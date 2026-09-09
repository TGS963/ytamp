use super::*;

pub(super) fn play_click(playing: bool) -> Option<Action> {
    (!playing).then_some(Action::PlayToggled)
}

pub(super) fn pause_click(playing: bool) -> Option<Action> {
    playing.then_some(Action::PlayToggled)
}

pub(super) fn stop_click(playing: bool) -> [Option<Action>; 2] {
    [
        playing.then_some(Action::PlayToggled),
        Some(Action::SeekRequested(Duration::ZERO)),
    ]
}

pub(super) fn windows_buttons(view: &mut View, shell: &mut WinampShell) {
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

/// Play, pause, and stop: the three buttons whose meaning depends on
/// whether the queue is already playing. The buttons never latch, as
/// in Winamp. Only a held pointer shows the pressed sprite, and the
/// lamp next to the time shows the state.
pub(super) fn play_pause_stop(view: &mut View, state: &State, out: &mut Vec<Action>) {
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
pub(super) fn push_if_clicked(
    response: egui::Response,
    out: &mut Vec<Action>,
    action: Option<Action>,
) {
    if response.clicked() {
        out.extend(action);
    }
}

/// Previous, next, and eject: buttons whose meaning never depends on
/// the current status.
pub(super) fn simple_transport(view: &mut View, out: &mut Vec<Action>) {
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
pub(super) fn mini_button(
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
pub(super) fn mini_transport(view: &mut View, state: &State, out: &mut Vec<Action>) {
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

pub(super) fn shuffle_repeat(view: &mut View, state: &State, out: &mut Vec<Action>) {
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
pub(super) fn clutter_bar(view: &mut View, state: &State, out: &mut Vec<Action>) {
    view.sprite(sprites::CLUTTER_BAR, layout::CLUTTER_BAR);
    let options = view
        .interact(layout::CLUTTER_O, "clutter-o", Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if options.is_pointer_button_down_on() {
        view.sprite(sprites::CLUTTER_O_LIT, layout::CLUTTER_O);
    }
    let unit = view.unit;
    super::menus::options_menu(egui::Popup::menu(&options), view.skin, state, unit, out);
}
