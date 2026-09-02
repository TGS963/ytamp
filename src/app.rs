//! The imperative shell around the functional core.
//!
//! Each frame: draw the views, collect their actions, drain the actions
//! that arrived from the effect runtime, reduce them all, and hand the
//! resulting effects to the runtime.

use std::sync::mpsc::Receiver;

use crate::core::action::Action;
use crate::core::state::State;
use crate::core::update::update;
use crate::media_keys::MediaKeys;
use crate::runtime::EffectRuntime;
use crate::theme::{ColorRole, DefaultTheme, Theme};
use crate::ui;

pub struct App {
    state: State,
    theme: Box<dyn Theme>,
    runtime: EffectRuntime,
    incoming: Receiver<Action>,
    media_keys: MediaKeys,
    rng: fastrand::Rng,
    winamp: ui::winamp::WinampShell,
}

impl App {
    pub fn new(runtime: EffectRuntime, incoming: Receiver<Action>, media_keys: MediaKeys) -> Self {
        Self {
            state: State::default(),
            theme: Box::new(DefaultTheme),
            runtime,
            incoming,
            media_keys,
            rng: fastrand::Rng::new(),
            winamp: ui::winamp::WinampShell::new(),
        }
    }

    /// Opens the Winamp skin window as a borderless egui viewport, and
    /// draws it through the ported skin view. Hides the main window
    /// while it is open, since the two are one app wearing two looks,
    /// not two windows at once.
    fn winamp_window(&mut self, ctx: &egui::Context) -> Vec<Action> {
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Visible(false));
        let size = ui::winamp::window_size_points(
            self.winamp.shade,
            self.state.winamp.scale,
            ctx.pixels_per_point(),
        );
        let mut builder = egui::ViewportBuilder::default()
            .with_title("ytamp")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_max_inner_size(size);
        if self.state.winamp.on_top {
            builder = builder.with_always_on_top();
        }
        let mut actions = Vec::new();
        let state = &self.state;
        let winamp = &mut self.winamp;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("winamp"),
            builder,
            |ui, _class| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        ui::winamp::show(ui, state, winamp, &mut actions);
                    });
                actions.extend(ui::winamp::dropped_skins(ui.ctx()));
                if ui.ctx().input(|input| input.viewport().close_requested()) {
                    actions.push(Action::WinampToggled);
                }
            },
        );
        actions
    }

    pub fn queue_action(&mut self, action: Action) {
        self.reduce(vec![action]);
    }

    fn reduce(&mut self, actions: Vec<Action>) {
        for action in actions {
            let Some(action) = self.deliver_to_shell(action) else {
                continue;
            };
            let rng = &mut self.rng;
            let mut random_below = |n: usize| rng.usize(0..n.max(1));
            let effects = update(&mut self.state, action, &mut random_below);
            for effect in effects {
                self.runtime.run(effect);
            }
        }
    }

    /// `SkinLoaded` never reaches the reducer: the decoded skin lives
    /// only in the shell, never in `State`. A successful load wears
    /// the skin here and stops; a failure becomes a plain notice,
    /// which the reducer already knows how to show.
    fn deliver_to_shell(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::SkinLoaded(Ok(skin)) => {
                self.winamp.wear(skin);
                None
            }
            Action::SkinLoaded(Err(message)) => Some(Action::NoticePosted(message)),
            other => Some(other),
        }
    }
}

const SESSION_STORAGE_KEY: &str = "session";

impl App {
    pub fn restore_session(&mut self, storage: Option<&dyn eframe::Storage>) {
        let Some(json) = storage.and_then(|storage| storage.get_string(SESSION_STORAGE_KEY)) else {
            return;
        };
        if let Some(session) = crate::core::session::SavedSession::from_json(&json) {
            self.queue_action(Action::SessionRestored(session));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut actions: Vec<Action> = self.incoming.try_iter().collect();
        actions.extend(ui::view(ui, &self.state, self.theme.as_ref()));
        actions.extend(ui::winamp::dropped_skins(ui.ctx()));
        let ctx = ui.ctx().clone();
        if self.state.winamp.open {
            actions.extend(self.winamp_window(&ctx));
        } else {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Visible(true));
        }
        self.reduce(actions);
        self.media_keys.sync(&self.state);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let session = crate::core::session::SavedSession::capture(&self.state);
        if let Some(json) = session.to_json() {
            storage.set_string(SESSION_STORAGE_KEY, json);
        }
    }

    /// The color eframe clears the window with before any panel paints.
    /// It shows through any gap a panel frame does not cover, so it
    /// must match the theme, not the library's semi-transparent default.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme
            .color(ColorRole::PageBackground)
            .to_normalized_gamma_f32()
    }
}
