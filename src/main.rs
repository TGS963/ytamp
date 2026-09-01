mod app;
mod core;
mod runtime;
mod theme;
mod ui;

use std::sync::mpsc;

fn main() -> eframe::Result {
    env_logger::init();
    let (action_sender, action_receiver) = mpsc::channel();
    let effect_runtime = runtime::EffectRuntime::new(action_sender);
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
        Box::new(move |_cc| Ok(Box::new(app::App::new(effect_runtime, action_receiver)))),
    )
}
