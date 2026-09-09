//! Pure views. They read the state, draw, and return actions.
//! They never mutate the state and never talk to the network.

mod album;
mod artist;
mod components;
mod discovery;
mod equalizer;
mod home;
mod library;
mod listening_history;
mod now_playing;
mod player_bar;
mod playlist;
mod queue;
mod rows;
mod search;
mod sign_in;
pub mod skin_browser;
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
        if state.playback.queue.current().is_some() {
            keyboard_shortcuts(ui, &mut actions);
            player_bar::view(ui, state, theme, &mut actions);
        }
        sign_in::view(ui, state, theme, &mut actions);
        components::focus_outline(ui, theme);
        return actions;
    }
    keyboard_shortcuts(ui, &mut actions);
    player_bar::view(ui, state, theme, &mut actions);
    sidebar(ui, state, theme, &mut actions);
    if state.queue_open {
        queue::view(ui, state, theme, &mut actions);
    }
    notices(ui, state, theme, &mut actions);
    page(ui, state, theme, &mut actions);
    create_playlist_dialog(ui, state, &mut actions);
    components::focus_outline(ui, theme);
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
pub(crate) fn keyboard_shortcuts(ui: &Ui, out: &mut Vec<Action>) {
    if ui.input(winamp_shortcut_pressed) {
        out.push(Action::WinampToggled);
    }
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
    visuals.slider_trailing_fill = true;
    visuals.selection.stroke.color = theme.color(ColorRole::TextPrimary);
    visuals.widgets.inactive.weak_bg_fill = theme.color(ColorRole::Surface);
    visuals.widgets.inactive.bg_fill = theme.color(ColorRole::Surface);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1., theme.color(ColorRole::Border));
    visuals.widgets.hovered.weak_bg_fill = theme.color(ColorRole::RowHover);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1., theme.color(ColorRole::Accent));
    visuals.widgets.active.bg_fill = theme.color(ColorRole::Accent);
    visuals.widgets.active.weak_bg_fill = theme.color(ColorRole::AccentSoft);
    visuals.override_text_color = Some(theme.color(ColorRole::TextPrimary));
    ui.ctx().set_visuals(visuals);
    // A selectable label senses clicks for text selection, so a
    // right-click on a title reached the label instead of the row
    // and its menu. A player has no use for text selection.
    ui.ctx().all_styles_mut(|style| {
        style.interaction.selectable_labels = false;
        style.spacing.button_padding = egui::vec2(10., 6.);
        style.spacing.item_spacing = egui::vec2(8., 6.);
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(7);
    });
}

/// A side- or bottom-panel frame filled with the given role, in place
/// of egui's shared `panel_fill`. This is how the sidebar and the
/// player bar earn a tone distinct from the page behind them.
pub(super) fn panel_frame(ui: &Ui, theme: &dyn Theme, role: ColorRole) -> egui::Frame {
    egui::Frame::side_top_panel(ui.style()).fill(theme.color(role))
}

fn sidebar(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let frame = panel_frame(ui, theme, ColorRole::PanelBackground).inner_margin(12);
    let width = if ui.ctx().content_rect().width() < 900. {
        156.
    } else {
        theme.metric(MetricRole::SidebarWidth)
    };
    egui::Panel::left("sidebar")
        .exact_size(width)
        .frame(frame)
        .show(ui, |ui| {
            ui.add_space(16.);
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(28., 28.), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 8, theme.color(ColorRole::Accent));
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![
                        rect.min + egui::vec2(10., 7.),
                        rect.min + egui::vec2(21., 14.),
                        rect.min + egui::vec2(10., 21.),
                    ],
                    theme.color(ColorRole::OnAccent),
                    egui::Stroke::NONE,
                ));
                ui.label(theme.label(TextRole::Heading, "ytamp").strong());
            });
            ui.add_space(28.);
            nav_item(
                ui,
                theme,
                "Home",
                matches!(
                    state.page,
                    Page::Home | Page::Discovery(_) | Page::DiscoveryShelf(_)
                ),
                Page::Home,
                out,
            );
            nav_item(
                ui,
                theme,
                "Now playing",
                state.page == Page::NowPlaying,
                Page::NowPlaying,
                out,
            );
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
            nav_item(
                ui,
                theme,
                "History",
                state.page == Page::ListeningHistory,
                Page::ListeningHistory,
                out,
            );
            let compact = ui.ctx().content_rect().height() < 700.;
            let footer_height = if compact { 48. } else { 150. };
            sidebar_playlists(ui, state, theme, out, footer_height);
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.add_space(10.);
                if compact {
                    ui.menu_button("Player & account", |ui| {
                        if ui.button("Skin collection").clicked() {
                            out.push(Action::SkinBrowserToggled);
                            ui.close();
                        }
                        if ui.button("Winamp mode").clicked() {
                            out.push(Action::WinampToggled);
                            ui.close();
                        }
                        ui.separator();
                        ui.weak("Connected to YouTube");
                        if ui.button("Sign out").clicked() {
                            out.push(Action::SignOutRequested);
                            ui.close();
                        }
                    });
                    return;
                }
                ui.menu_button("Account", |ui| {
                    ui.label("Connected to YouTube");
                    if ui.button("Sign out").clicked() {
                        out.push(Action::SignOutRequested);
                        ui.close();
                    }
                });
                if ui
                    .add_sized(
                        [ui.available_width(), 32.],
                        components::quiet("Winamp mode").right_text("↗"),
                    )
                    .clicked()
                {
                    out.push(Action::WinampToggled);
                }
                if ui
                    .add_sized(
                        [ui.available_width(), 32.],
                        components::quiet("Skin collection").right_text(""),
                    )
                    .clicked()
                {
                    out.push(Action::SkinBrowserToggled);
                }
                ui.separator();
            });
        });
}

fn sidebar_playlists(
    ui: &mut Ui,
    state: &State,
    theme: &dyn Theme,
    out: &mut Vec<Action>,
    footer_height: f32,
) {
    if ui.available_height() > footer_height + 100. {
        ui.add_space(16.);
        ui.label(theme.secondary_label(TextRole::Caption, "YOUR PLAYLISTS"));
        ui.add_space(8.);
        if let Some(playlists) = state.library.playlists.loaded() {
            egui::ScrollArea::vertical()
                .id_salt("sidebar-playlists")
                .max_height((ui.available_height() - footer_height - 12.).max(0.))
                .show(ui, |ui| {
                    for playlist in playlists.iter().take(8) {
                        if ui
                            .add_sized(
                                [ui.available_width(), 30.],
                                components::quiet(
                                    theme.secondary_label(TextRole::Caption, &playlist.title),
                                )
                                .right_text("")
                                .truncate(),
                            )
                            .on_hover_text(&playlist.title)
                            .clicked()
                        {
                            out.push(Action::PlaylistOpened(playlist.id.clone()));
                        }
                    }
                });
        }
    }
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
    let color = if active {
        theme.color(ColorRole::TextPrimary)
    } else {
        theme.color(ColorRole::TextSecondary)
    };
    let button = egui::Button::new(theme.label(TextRole::Body, text).color(color).strong())
        .fill(if active {
            theme.color(ColorRole::AccentSoft)
        } else {
            egui::Color32::TRANSPARENT
        })
        .stroke(egui::Stroke::NONE)
        .right_text(if active { "•" } else { "" });
    if ui.add_sized([ui.available_width(), 40.], button).clicked() {
        out.push(Action::NavigatedTo(target));
    }
}

fn page(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let frame = egui::Frame::central_panel(ui.style())
        .fill(theme.color(ColorRole::PageBackground))
        .inner_margin(24);
    let context = row_context(state);
    egui::CentralPanel::default_margins()
        .frame(frame)
        .show(ui, |ui| {
            ui.add_space(theme.metric(MetricRole::PagePadding));
            back_button(ui, state, theme, out);
            ui.push_id(format!("page:{:?}", state.page), |ui| match &state.page {
                Page::SignIn | Page::Search => search::view(ui, state, theme, &context, out),
                Page::NowPlaying => now_playing::view(ui, state, theme, out),
                Page::DiscoveryShelf(_) => discovery::shelf_page(ui, state, theme, &context, out),
                Page::Discovery(_) => discovery::collection(ui, state, theme, &context, out),
                Page::Home => home::view(ui, state, theme, &context, out),
                Page::Library => library::view(ui, state, theme, &context, out),
                Page::ListeningHistory => listening_history::view(ui, state, theme, &context, out),
                Page::Playlist(_) => playlist::view(ui, state, theme, &context, out),
                Page::Artist(_) => artist::view(ui, state, theme, &context, out),
                Page::Album(_) => album::view(ui, state, theme, &context, out),
            });
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
    if state.notices.is_empty() && state.playback.error.is_none() && !state.discovery.radio_loading
    {
        return;
    }
    egui::Panel::top("notices").show(ui, |ui| {
        if state.discovery.radio_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Starting radio…");
                if ui.button("Cancel").clicked() {
                    out.push(Action::RadioStartCancelled);
                }
            });
        }
        if let Some(error) = &state.playback.error {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(theme.color(ColorRole::Danger), error);
                if ui.button("Retry playback").clicked() {
                    out.push(Action::PlaybackRetryRequested);
                }
            });
        }
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

pub(crate) mod queue_edit;

pub mod lyrics;
