use std::sync::mpsc;

use ytamp::core::action::Action;
use ytamp::{app, auth, media_keys, runtime};

fn main() -> eframe::Result {
    // rustypipe logs an ERROR for every client the chain falls back
    // from, which reads as a failure while the song plays fine. The
    // chain already reports real failures, so rustypipe stays quiet
    // unless RUST_LOG says otherwise.
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,rustypipe=off"),
    )
    .init();
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
            let repaint_ctx = creation.egui_ctx.clone();
            let effect_runtime = runtime::EffectRuntime::new(action_sender.clone(), move || {
                repaint_ctx.request_repaint()
            });
            let key_ctx = creation.egui_ctx.clone();
            let media_keys = media_keys::MediaKeys::attach(move |action| {
                if action_sender.send(action).is_ok() {
                    key_ctx.request_repaint();
                }
            });
            let mut app = app::App::new(effect_runtime, action_receiver, media_keys);
            app.restore_session(creation.storage);
            if let Some(method) = auth::load_auth_method() {
                app.queue_action(Action::StoredAuthFound(method));
            }
            Ok(Box::new(app))
        }),
    )
}
