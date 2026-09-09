//! OAuth sign-in and recovery. Network retries never discard saved credentials.
use crate::{
    core::{
        action::Action,
        sign_in::SignInFailure,
        state::{AuthState, State},
    },
    theme::{ColorRole, TextRole, Theme},
};
use egui::Ui;
pub fn view(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    egui::CentralPanel::default_margins()
        .frame(
            egui::Frame::new()
                .fill(theme.color(ColorRole::PageBackground))
                .inner_margin(24),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let width = ui.available_width().min(520.);
                let margin = ((ui.available_width() - width) / 2.).max(0.);
                ui.add_space((ui.available_height() * 0.12).clamp(16., 72.));
                ui.horizontal(|ui| {
                    ui.add_space(margin);
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, 0.),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::Frame::new()
                                .fill(theme.color(ColorRole::Surface))
                                .stroke(egui::Stroke::new(1., theme.color(ColorRole::Border)))
                                .corner_radius(12)
                                .inner_margin(24)
                                .show(ui, |ui| {
                                    ui.set_width((width - 50.).max(120.));
                                    content(ui, state, theme, out);
                                });
                        },
                    );
                });
            });
        });
}
fn content(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    if ui.button("Play local files").clicked() {
        out.push(Action::LocalModeOpened);
    }
    ui.add_space(12.);
    ui.label(theme.secondary_label(TextRole::Caption, "YTAMP"));
    ui.add_space(10.);
    let title = match state.auth {
        AuthState::ConnectionFailed => "Let’s reconnect",
        AuthState::Expired => "Sign in again",
        AuthState::Verifying => "Connecting to YouTube",
        _ => "Your music starts here",
    };
    ui.label(theme.label(TextRole::Title, title).strong());
    ui.add_space(12.);
    match &state.auth {
        AuthState::Verifying => pending(ui, state, theme, out),
        AuthState::ConnectionFailed => {
            ui.label(if state.sign_in.saved_account {
                SignInFailure::Connection.message()
            } else {
                "We couldn’t connect to Google. Check your connection and try again."
            });
            ui.add_space(16.);
            if state.sign_in.saved_account {
                if primary(ui, theme, "Try again").clicked() {
                    out.push(Action::SignInRetryRequested);
                }
            } else {
                setup(ui, state, theme, out);
            }
        }
        AuthState::Expired => {
            ui.label(SignInFailure::Expired.message());
            ui.add_space(16.);
            if !state.sign_in.client_id_draft.is_empty()
                && !state.sign_in.client_secret_draft.is_empty()
            {
                if primary(ui, theme, "Sign in with Google").clicked() {
                    out.push(Action::OAuthStartRequested);
                }
                ui.add_space(12.);
                ui.collapsing("Change OAuth setup", |ui| setup(ui, state, theme, out));
            } else {
                setup(ui, state, theme, out);
            }
        }
        _ => {
            ui.label(theme.secondary_label(TextRole::Body, "Connect your Google account to bring your music, playlists, and recommendations together."));
            ui.add_space(16.);
            setup(ui, state, theme, out);
            if let AuthState::Failed(message) = &state.auth {
                ui.add_space(12.);
                ui.colored_label(theme.color(ColorRole::Danger), message);
            }
        }
    }
}

fn primary(ui: &mut Ui, theme: &dyn Theme, label: &str) -> egui::Response {
    ui.add(super::components::primary(theme, label))
}
fn setup(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    ui.label(theme.secondary_label(TextRole::Caption,"One-time setup: use your Google OAuth client for TVs and Limited Input devices, with YouTube Data API v3 enabled."));
    ui.add_space(12.);
    field(
        ui,
        "Client ID",
        &state.sign_in.client_id_draft,
        false,
        out,
        Action::OAuthClientIdChanged,
    );
    field(
        ui,
        "Client secret",
        &state.sign_in.client_secret_draft,
        true,
        out,
        Action::OAuthClientSecretChanged,
    );
    ui.add_space(12.);
    if primary(ui, theme, "Sign in with Google").clicked() {
        out.push(Action::OAuthStartRequested);
    }
}
fn field(
    ui: &mut Ui,
    label: &str,
    value: &str,
    secret: bool,
    out: &mut Vec<Action>,
    action: impl Fn(String) -> Action,
) {
    ui.label(label);
    let mut draft = value.to_owned();
    if ui
        .add(
            egui::TextEdit::singleline(&mut draft)
                .password(secret)
                .desired_width(f32::INFINITY)
                .margin(egui::vec2(10., 8.)),
        )
        .changed()
    {
        out.push(action(draft));
    }
    ui.add_space(8.);
}
fn pending(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    if let Some(url) = &state.sign_in.oauth_url {
        ui.label("Finish signing in on Google’s page, then return here.");
        ui.add_space(12.);
        if let Some(code) = user_code_of(url) {
            ui.horizontal(|ui| {
                ui.label(theme.label(TextRole::Heading, &code));
                if ui.small_button("Copy code").clicked() {
                    ui.ctx().copy_text(code);
                }
            });
        }
        ui.hyperlink_to("Open Google sign-in", url);
    } else {
        ui.label(if state.sign_in.saved_account {
            "Checking your saved sign-in…"
        } else {
            "Starting Google sign-in…"
        });
    }
    ui.add_space(12.);
    ui.horizontal(|ui| {
        ui.spinner();
        ui.weak("This usually takes a few seconds.");
    });
    if ui
        .add(super::components::secondary(theme, "Cancel"))
        .clicked()
    {
        out.push(Action::SignInCancelled);
    }
}
fn user_code_of(url: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    query
        .split('&')
        .find_map(|p| p.strip_prefix("user_code="))
        .map(|s| s.replace("%2D", "-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text_rect(shape: &egui::Shape, needle: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == needle => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_rect(shape, needle)),
            _ => None,
        }
    }
    #[test]
    fn saved_account_recovery_fits_and_retry_is_clickable_at_all_window_sizes() {
        for size in [[700., 480.], [1100., 720.], [1710., 1073.]] {
            let mut state = State {
                auth: AuthState::ConnectionFailed,
                ..Default::default()
            };
            state.sign_in.saved_account = true;
            let ctx = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
            let frame = |events| {
                let mut actions = vec![];
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        actions = super::super::view(ui, &state, &crate::theme::DefaultTheme);
                    },
                );
                output.textures_delta.clear();
                (output, actions)
            };
            frame(vec![]);
            let (output, _) = frame(vec![]);
            let find = |needle| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| text_rect(&shape.shape, needle))
            };
            let button = find("Try again").expect("retry must be visible");
            assert!(screen.contains_rect(button));
            assert!(find("Client secret").is_none());
            assert!(find("Sign in with Google").is_none());
            let pos = button.center();
            frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ]);
            let (_, actions) = frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }]);
            assert!(
                actions
                    .iter()
                    .any(|action| matches!(action, Action::SignInRetryRequested))
            );
        }
    }
}
