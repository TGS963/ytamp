//! Executes effects and sends result actions back to the app.
//!
//! This commit ships a stub that only logs. The API layer and the
//! player engine take over in later commits.

use std::sync::mpsc::Sender;

use crate::core::action::Action;
use crate::core::effect::Effect;

pub struct EffectRuntime {
    #[allow(
        dead_code,
        reason = "the API layer and the player use this in later commits"
    )]
    actions: Sender<Action>,
}

impl EffectRuntime {
    pub fn new(actions: Sender<Action>) -> Self {
        Self { actions }
    }

    pub fn run(&self, effect: Effect) {
        log::info!("effect (not yet executed): {effect:?}");
    }
}
