//! The sign-in page. The main path is the OAuth device flow with the
//! user's own Google Cloud client. Cookie paste stays as the fallback.

use egui::Ui;

use crate::core::action::Action;
use crate::core::state::{AuthState, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

pub fn view(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    egui::CentralPanel::default_margins().show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(theme.metric(MetricRole::PagePadding) * 2.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(560.0);
                ui.label(theme.label(TextRole::Title, "Sign in to YouTube Music"));
                ui.add_space(theme.metric(MetricRole::GapLarge));
                oauth_section(ui, state, theme, out);
                ui.add_space(theme.metric(MetricRole::GapLarge));
                ui.separator();
                cookie_section(ui, state, theme, out);
                ui.add_space(theme.metric(MetricRole::GapSmall));
                status_line(ui, state, theme);
            });
        });
    });
}

fn oauth_section(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    ui.label(theme.label(TextRole::Heading, "Sign in with Google"));
    for line in [
        "Uses your own Google Cloud OAuth client of the type",
        "\"TVs and Limited Input devices\". One-time setup, then the",
        "sign-in refreshes itself.",
    ] {
        ui.label(theme.secondary_label(TextRole::Body, line));
    }
    ui.add_space(theme.metric(MetricRole::GapSmall));
    labeled_field(
        ui,
        state.sign_in.client_id_draft.clone(),
        "Client id",
        false,
        out,
        |draft| Action::OAuthClientIdChanged(draft),
    );
    labeled_field(
        ui,
        state.sign_in.client_secret_draft.clone(),
        "Client secret",
        true,
        out,
        Action::OAuthClientSecretChanged,
    );
    ui.add_space(theme.metric(MetricRole::GapSmall));
    match (&state.auth, &state.sign_in.oauth_url) {
        (AuthState::Verifying, Some(url)) => {
            ui.hyperlink_to("Open the Google sign-in page", url);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(theme.secondary_label(TextRole::Body, "Waiting for the sign-in…"));
            });
        }
        (AuthState::Verifying, None) => {
            ui.spinner();
        }
        _ => {
            if ui.button("Sign in with Google").clicked() {
                out.push(Action::OAuthStartRequested);
            }
        }
    }
}

fn labeled_field(
    ui: &mut Ui,
    mut draft: String,
    label: &str,
    secret: bool,
    out: &mut Vec<Action>,
    action: impl Fn(String) -> Action,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        let field = egui::TextEdit::singleline(&mut draft)
            .password(secret)
            .desired_width(f32::INFINITY);
        if ui.add(field).changed() {
            out.push(action(draft));
        }
    });
}

fn cookie_section(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    ui.label(theme.label(TextRole::Heading, "Fallback: cookie paste"));
    for line in [
        "1. In the browser's Network tab, right-click a music.youtube.com",
        "   browse request and select Copy as cURL.",
        "2. Paste the whole text below.",
    ] {
        ui.label(theme.secondary_label(TextRole::Body, line));
    }
    let mut draft = state.sign_in.draft.clone();
    let edit = egui::TextEdit::multiline(&mut draft)
        .hint_text("Cookie header value, or the whole Copy-as-cURL text")
        .desired_rows(4)
        .desired_width(f32::INFINITY);
    let changed = egui::ScrollArea::vertical()
        .id_salt("cookie_paste")
        .max_height(120.0)
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
        let enabled = !state.sign_in.draft.trim().is_empty();
        if ui
            .add_enabled(enabled, egui::Button::new("Sign in with cookies"))
            .clicked()
        {
            out.push(Action::CookiesSubmitted);
        }
    });
}

fn status_line(ui: &mut Ui, state: &State, theme: &dyn Theme) {
    if let AuthState::Failed(message) = &state.auth {
        ui.colored_label(theme.color(ColorRole::Danger), message);
    }
}
