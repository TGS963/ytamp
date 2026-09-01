//! The sign-in page: paste the cookies of a music.youtube.com session.

use egui::Ui;

use crate::core::action::Action;
use crate::core::state::{AuthState, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

pub fn view(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    egui::CentralPanel::default_margins().show(ui, |ui| {
        ui.add_space(theme.metric(MetricRole::PagePadding) * 3.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(560.0);
            ui.label(theme.label(TextRole::Title, "Sign in to YouTube Music"));
            ui.add_space(theme.metric(MetricRole::GapLarge));
            instructions(ui, theme);
            ui.add_space(theme.metric(MetricRole::GapLarge));
            cookie_editor(ui, state, out);
            ui.add_space(theme.metric(MetricRole::GapSmall));
            status_line(ui, state, theme, out);
        });
    });
}

fn instructions(ui: &mut egui::Ui, theme: &dyn Theme) {
    for line in [
        "1. Open music.youtube.com in your browser and log in.",
        "2. Open the developer tools and select the Network tab.",
        "3. Type 'browse' in the filter and right-click a browse request.",
        "4. Select Copy as cURL and paste the whole text below.",
        "5. Copy the X-Goog-AuthUser request header into the account field.",
    ] {
        ui.label(theme.secondary_label(TextRole::Body, line));
    }
    ui.label(theme.secondary_label(
        TextRole::Caption,
        "Copy as cURL avoids the shortened display text that a direct copy takes. \
         The account field picks the Google account when several are signed in. Empty means 0.",
    ));
}

fn cookie_editor(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    let mut draft = state.sign_in.draft.clone();
    let edit = egui::TextEdit::multiline(&mut draft)
        .hint_text("Cookie header value, or the whole Copy-as-cURL text")
        .desired_rows(6)
        .desired_width(f32::INFINITY);
    let changed = egui::ScrollArea::vertical()
        .max_height(160.0)
        .show(ui, |ui| ui.add(edit).changed())
        .inner;
    if changed {
        out.push(Action::CookieDraftChanged(draft));
    }
    ui.horizontal(|ui| {
        ui.label("Account (X-Goog-AuthUser):");
        let mut authuser = state.sign_in.authuser_draft.clone();
        let field = egui::TextEdit::singleline(&mut authuser)
            .hint_text("0")
            .desired_width(40.0);
        if ui.add(field).changed() {
            out.push(Action::AuthUserDraftChanged(authuser));
        }
    });
}

fn status_line(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    match &state.auth {
        AuthState::Verifying => {
            ui.spinner();
        }
        AuthState::Failed(message) => {
            ui.colored_label(theme.color(ColorRole::Danger), message);
            sign_in_button(ui, state, out);
        }
        _ => sign_in_button(ui, state, out),
    }
}

fn sign_in_button(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    let enabled = !state.sign_in.draft.trim().is_empty();
    if ui
        .add_enabled(enabled, egui::Button::new("Sign in"))
        .clicked()
    {
        out.push(Action::CookiesSubmitted);
    }
}
