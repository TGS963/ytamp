mod api;
mod app;
mod auth;
mod core;
mod player;
mod runtime;
mod stream;
mod theme;
mod ui;

use std::sync::mpsc;

use crate::core::action::Action;

fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([700.0, 480.0])
            .with_title("ytamp"),
        ..Default::default()
    };
    eframe::run_native(
        "ytamp",
        options,
        Box::new(|creation| {
            let (action_sender, action_receiver) = mpsc::channel();
            let egui_ctx = creation.egui_ctx.clone();
            let effect_runtime =
                runtime::EffectRuntime::new(action_sender, move || egui_ctx.request_repaint());
            let mut app = app::App::new(effect_runtime, action_receiver);
            if let Some(cookies) = auth::load_cookies() {
                app.queue_action(Action::StoredCookiesFound(cookies));
            }
            Ok(Box::new(app))
        }),
    )
}
