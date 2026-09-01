//! The imperative shell around the functional core.
//!
//! Each frame: draw the views, collect their actions, drain the actions
//! that arrived from the effect runtime, reduce them all, and hand the
//! resulting effects to the runtime.

use std::sync::mpsc::Receiver;

use crate::core::action::Action;
use crate::core::state::State;
use crate::core::update::update;
use crate::runtime::EffectRuntime;
use crate::theme::{DefaultTheme, Theme};
use crate::ui;

pub struct App {
    state: State,
    theme: Box<dyn Theme>,
    runtime: EffectRuntime,
    incoming: Receiver<Action>,
    rng: fastrand::Rng,
}

impl App {
    pub fn new(runtime: EffectRuntime, incoming: Receiver<Action>) -> Self {
        Self {
            state: State::default(),
            theme: Box::new(DefaultTheme),
            runtime,
            incoming,
            rng: fastrand::Rng::new(),
        }
    }

    pub fn queue_action(&mut self, action: Action) {
        self.reduce(vec![action]);
    }

    fn reduce(&mut self, actions: Vec<Action>) {
        let rng = &mut self.rng;
        let mut random_below = |n: usize| rng.usize(0..n.max(1));
        for action in actions {
            let effects = update(&mut self.state, action, &mut random_below);
            for effect in effects {
                self.runtime.run(effect);
            }
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
        self.reduce(actions);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let session = crate::core::session::SavedSession::capture(&self.state);
        if let Some(json) = session.to_json() {
            storage.set_string(SESSION_STORAGE_KEY, json);
        }
    }
}
