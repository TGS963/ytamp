//! Pure views. They read the state, draw, and return actions.
//! They never mutate the state and never talk to the network.

mod library;
mod player_bar;
mod playlist;
mod queue;
mod rows;
mod search;
mod sign_in;

use egui::Ui;

use crate::core::action::Action;
use crate::core::state::{AuthState, Page, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

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
    actions
}

/// Global shortcuts. They stay quiet while a text field has the focus.
fn keyboard_shortcuts(ui: &Ui, out: &mut Vec<Action>) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let shortcuts: [(egui::Key, fn() -> Action); 4] = [
        (egui::Key::Space, || Action::PlayToggled),
        (egui::Key::ArrowRight, || Action::NextPressed),
        (egui::Key::ArrowLeft, || Action::PreviousPressed),
        (egui::Key::Q, || Action::QueuePanelToggled),
    ];
    ui.input(|input| {
        for (key, action) in shortcuts {
            if input.key_pressed(key) {
                out.push(action());
            }
        }
    });
}

fn apply_page_style(ui: &mut Ui, theme: &dyn Theme) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = theme.color(ColorRole::PageBackground);
    visuals.selection.bg_fill = theme.color(ColorRole::Accent);
    ui.ctx().set_visuals(visuals);
}

fn sidebar(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    egui::Panel::left("sidebar")
        .exact_size(theme.metric(MetricRole::SidebarWidth))
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
    egui::CentralPanel::default_margins().show(ui, |ui| {
        ui.add_space(theme.metric(MetricRole::PagePadding));
        match &state.page {
            Page::SignIn | Page::Search => search::view(ui, state, theme, out),
            Page::Library => library::view(ui, state, theme, out),
            Page::Playlist(_) => playlist::view(ui, state, theme, out),
        }
    });
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
