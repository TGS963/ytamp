//! Pure views. They read the state, draw, and return actions.
//! They never mutate the state and never talk to the network.

mod album;
mod artist;
mod library;
mod player_bar;
mod playlist;
mod queue;
mod rows;
mod search;
mod sign_in;
pub mod winamp;

use egui::Ui;

use crate::core::action::Action;
use crate::core::state::{AuthState, Dialog, Page, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};
use rows::RowContext;

pub fn view(ui: &mut Ui, state: &State, theme: &dyn Theme) -> Vec<Action> {
    let mut actions = Vec::new();
    apply_page_style(ui, theme);
    if state.auth != AuthState::SignedIn {
        sign_in::view(ui, state, theme, &mut actions);
        return actions;
    }
    keyboard_shortcuts(ui, &mut actions);
    player_bar::view(ui, state, theme, &mut actions);
    sidebar(ui, state, theme, &mut actions);
    if state.queue_open {
        queue::view(ui, state, theme);
    }
    notices(ui, state, theme, &mut actions);
    page(ui, state, theme, &mut actions);
    create_playlist_dialog(ui, state, &mut actions);
    actions
}

/// The row context every track list on the page shares: the liked
/// list, the library's playlists, and the page itself. Built once here
/// rather than once per list, let alone once per row.
fn row_context(state: &State) -> RowContext<'_> {
    RowContext {
        liked: &state.library.liked,
        playlists: state
            .library
            .playlists
            .loaded()
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        page: &state.page,
    }
}

/// Global shortcuts. They stay quiet while a text field has the focus.
fn keyboard_shortcuts(ui: &Ui, out: &mut Vec<Action>) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let shortcuts: [(egui::Key, fn() -> Action); 5] = [
        (egui::Key::Space, || Action::PlayToggled),
        (egui::Key::ArrowRight, || Action::NextPressed),
        (egui::Key::ArrowLeft, || Action::PreviousPressed),
        (egui::Key::Q, || Action::QueuePanelToggled),
        (egui::Key::Backspace, || Action::BackPressed),
    ];
    ui.input(|input| {
        if winamp_shortcut_pressed(input) {
            out.push(Action::WinampToggled);
        }
        for (key, action) in shortcuts {
            if input.key_pressed(key) {
                out.push(action());
            }
        }
    });
}

/// The Winamp window's shortcut: Ctrl+M everywhere but macOS, where
/// Ctrl+M is taken by window minimize, so it is Cmd+Shift+M there.
fn winamp_shortcut_pressed(input: &egui::InputState) -> bool {
    if cfg!(target_os = "macos") {
        input.modifiers.mac_cmd && input.modifiers.shift && input.key_pressed(egui::Key::M)
    } else {
        input.modifiers.ctrl && input.key_pressed(egui::Key::M)
    }
}

/// Sets the widget colors every panel shares (dark theme, accent
/// selection), and a page-background fallback for any panel that does
/// not name its own fill. Each panel below still names its own fill
/// through `panel_frame`, so this fallback rarely fires.
fn apply_page_style(ui: &mut Ui, theme: &dyn Theme) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = theme.color(ColorRole::PageBackground);
    visuals.window_fill = theme.color(ColorRole::PageBackground);
    visuals.selection.bg_fill = theme.color(ColorRole::Accent);
    ui.ctx().set_visuals(visuals);
}

/// A side- or bottom-panel frame filled with the given role, in place
/// of egui's shared `panel_fill`. This is how the sidebar and the
/// player bar earn a tone distinct from the page behind them.
pub(super) fn panel_frame(ui: &Ui, theme: &dyn Theme, role: ColorRole) -> egui::Frame {
    egui::Frame::side_top_panel(ui.style()).fill(theme.color(role))
}

fn sidebar(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let frame = panel_frame(ui, theme, ColorRole::PanelBackground);
    egui::Panel::left("sidebar")
        .exact_size(theme.metric(MetricRole::SidebarWidth))
        .frame(frame)
        .show(ui, |ui| {
            ui.add_space(theme.metric(MetricRole::PagePadding));
            nav_item(
                ui,
                theme,
                "Search",
                state.page == Page::Search,
                Page::Search,
                out,
            );
            nav_item(
                ui,
                theme,
                "Library",
                is_library_page(&state.page),
                Page::Library,
                out,
            );
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.add_space(theme.metric(MetricRole::PagePadding));
                if ui
                    .button("Sign out")
                    .on_hover_text("Clear the saved sign-in")
                    .clicked()
                {
                    out.push(Action::SignOutRequested);
                }
            });
        });
}

fn is_library_page(page: &Page) -> bool {
    matches!(page, Page::Library | Page::Playlist(_))
}

fn nav_item(
    ui: &mut egui::Ui,
    theme: &dyn Theme,
    text: &str,
    active: bool,
    target: Page,
    out: &mut Vec<Action>,
) {
    let label = if active {
        theme.label(TextRole::Heading, text)
    } else {
        theme.secondary_label(TextRole::Heading, text)
    };
    if ui
        .add(egui::Label::new(label).sense(egui::Sense::click()))
        .clicked()
    {
        out.push(Action::NavigatedTo(target));
    }
}

fn page(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let frame = egui::Frame::central_panel(ui.style()).fill(theme.color(ColorRole::PageBackground));
    let context = row_context(state);
    egui::CentralPanel::default_margins()
        .frame(frame)
        .show(ui, |ui| {
            ui.add_space(theme.metric(MetricRole::PagePadding));
            back_button(ui, state, theme, out);
            match &state.page {
                Page::SignIn | Page::Search => search::view(ui, state, theme, &context, out),
                Page::Library => library::view(ui, state, theme, &context, out),
                Page::Playlist(_) => playlist::view(ui, state, theme, &context, out),
                Page::Artist(_) => artist::view(ui, state, theme, &context, out),
                Page::Album(_) => album::view(ui, state, theme, &context, out),
            }
        });
}

/// A Back link above the page content, shown only while there is
/// somewhere to go back to.
fn back_button(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    if state.history.is_empty() {
        return;
    }
    if ui
        .add(
            egui::Label::new(theme.secondary_label(TextRole::Body, "< Back"))
                .sense(egui::Sense::click()),
        )
        .clicked()
    {
        out.push(Action::BackPressed);
    }
    ui.add_space(theme.metric(MetricRole::GapSmall));
}

fn notices(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    if state.notices.is_empty() {
        return;
    }
    egui::Panel::top("notices").show(ui, |ui| {
        for (index, notice) in state.notices.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.colored_label(theme.color(ColorRole::Danger), notice);
                if ui.small_button("Dismiss").clicked() {
                    out.push(Action::NoticeDismissed(index));
                }
            });
        }
    });
}

/// The "New playlist" window, drawn only while `state.dialog` holds a
/// `Dialog::CreatePlaylist`. Its text field takes the keyboard focus
/// as soon as it opens.
fn create_playlist_dialog(ui: &mut Ui, state: &State, out: &mut Vec<Action>) {
    let Some(Dialog::CreatePlaylist { title_draft, .. }) = &state.dialog else {
        return;
    };
    let mut draft = title_draft.clone();
    egui::Window::new("New playlist")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ui.ctx(), |ui| {
            create_playlist_dialog_contents(ui, &mut draft, out)
        });
}

/// The dialog's text field and its Create and Cancel buttons. Enter
/// creates the playlist; Escape cancels.
fn create_playlist_dialog_contents(ui: &mut Ui, draft: &mut String, out: &mut Vec<Action>) {
    let field = ui.add(egui::TextEdit::singleline(draft).hint_text("Playlist name"));
    if field.changed() {
        out.push(Action::CreatePlaylistDraftChanged(draft.clone()));
    }
    if ui.memory(|memory| memory.focused().is_none()) {
        field.request_focus();
    }
    let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
    ui.horizontal(|ui| {
        if ui.button("Create").clicked() || enter {
            out.push(Action::PlaylistCreateRequested(draft.clone()));
        }
        if ui.button("Cancel").clicked() || escape {
            out.push(Action::DialogDismissed);
        }
    });
}
