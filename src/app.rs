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
use crate::theme::{DefaultTheme, Theme};
use crate::ui;

pub struct App {
    state: State,
    theme: Box<dyn Theme>,
    runtime: EffectRuntime,
    incoming: Receiver<Action>,
    media_keys: MediaKeys,
    rng: fastrand::Rng,
    /// True while the Winamp window hides the main window. The show
    /// command goes out once, on the way back, not on every frame.
    main_window_hidden: bool,
    winamp: ui::winamp::WinampShell,
    winamp_created: bool,
    lyrics_created: bool,
    skin_browser_created: bool,
    skin_browser: ui::skin_browser::SkinBrowser,
    menu_bar: Option<crate::menu_bar::MenuBar>,
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
            main_window_hidden: false,
            winamp: ui::winamp::WinampShell::new(),
            winamp_created: false,
            lyrics_created: false,
            skin_browser_created: false,
            skin_browser: Default::default(),
            menu_bar: None,
        }
    }

    /// Opens the Winamp skin window as a borderless egui viewport, and
    /// draws it through the ported skin view. Hides the main window
    /// while it is open, since the two are one app wearing two looks,
    /// not two windows at once.
    fn winamp_window(&mut self, ctx: &egui::Context) -> Vec<Action> {
        // Retain the child even when hidden: destroying the current CGL view
        // can leave glutin with a context whose NSView has disappeared.
        let open = self.state.winamp.open;
        let size = ui::winamp::window_size_points(
            &self.winamp,
            self.state.winamp.scale,
            ctx.pixels_per_point(),
        );
        let mut builder = crate::branding::viewport()
            .with_title("ytamp")
            .with_visible(open)
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
                if !open {
                    return;
                }
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
            self.runtime
                .set_session_generation(self.state.session_generation);
            self.runtime
                .set_playback_generation(self.state.playback_generation);
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
            Action::SkinInstalled(result) => {
                self.skin_browser.invalidate();
                Some(Action::SkinInstalled(result))
            }
            Action::SkinLoaded(Ok(skin)) => {
                self.winamp.wear(skin);
                None
            }
            Action::SkinLoaded(Err(message)) if self.state.winamp.skin.is_some() => {
                self.reduce(vec![Action::SkinChosen(None)]);
                Some(Action::NoticePosted(format!(
                    "{message} The built-in skin is in use."
                )))
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
            self.queue_action(Action::SessionRestored(Box::new(session)));
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut actions: Vec<Action> = self.incoming.try_iter().collect();
        let menu_bar = self
            .menu_bar
            .get_or_insert_with(|| crate::menu_bar::MenuBar::new(ctx.clone()));
        for event in menu_bar.drain() {
            use crate::menu_bar::MenuAction;
            match event {
                MenuAction::PlayPause => actions.push(Action::PlayToggled),
                MenuAction::Previous => actions.push(Action::PreviousPressed),
                MenuAction::Next => actions.push(Action::NextPressed),
                MenuAction::Lyrics => {
                    menu_bar.show();
                    actions.push(Action::LyricsToggled);
                }
                MenuAction::Quit => {
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close)
                }
                MenuAction::Visibility => {
                    if menu_bar.toggle_visibility() {
                        let viewport = if self.state.winamp.open {
                            egui::ViewportId::from_hash_of("winamp")
                        } else {
                            egui::ViewportId::ROOT
                        };
                        ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Focus);
                    }
                }
            }
        }
        self.reduce(actions);
        self.media_keys.sync(&self.state);
        if let Some(menu) = &self.menu_bar {
            menu.sync(&self.state);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut actions = ui::view(ui, &self.state, self.theme.as_ref());
        actions.extend(ui::winamp::dropped_skins(ui.ctx()));
        let ctx = ui.ctx().clone();
        if self.state.winamp.open != self.main_window_hidden {
            ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::Visible(!self.state.winamp.open),
            );
            self.main_window_hidden = self.state.winamp.open;
        }
        self.winamp_created |= self.state.winamp.open;
        if self.winamp_created {
            actions.extend(self.winamp_window(&ctx));
        }
        self.skin_browser_created |= self.state.skin_browser_open;
        if self.skin_browser_created {
            let open = self.state.skin_browser_open;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("skin-browser"),
                crate::branding::viewport()
                    .with_title("Skins — ytamp")
                    .with_visible(open)
                    .with_inner_size([600., 650.])
                    .with_min_inner_size([320., 300.]),
                |ui, _| {
                    if open {
                        self.skin_browser.view(ui, &self.state, &mut actions);
                        if ui.ctx().input(|i| i.viewport().close_requested()) {
                            actions.push(Action::SkinBrowserToggled);
                        }
                    }
                },
            );
        }
        self.lyrics_created |= self.state.lyrics.open;
        if self.lyrics_created {
            let open = self.state.lyrics.open;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("lyrics"),
                crate::branding::viewport()
                    .with_title("Lyrics — ytamp")
                    .with_visible(open)
                    .with_inner_size([420., 520.])
                    .with_min_inner_size([260., 200.]),
                |ui, _| {
                    if open {
                        ui::lyrics::view(ui, &self.state, &mut actions);
                        if ui.ctx().input(|i| i.viewport().close_requested()) {
                            actions.push(Action::LyricsToggled);
                        }
                    }
                },
            );
        }
        self.reduce(actions);
        self.media_keys.sync(&self.state);
        if let Some(menu) = &self.menu_bar {
            menu.sync(&self.state);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let session = crate::core::session::SavedSession::capture(&self.state);
        if let Some(json) = session.to_json() {
            storage.set_string(SESSION_STORAGE_KEY, json);
        }
    }

    /// Panels paint the main window's background. Transparent clearing also
    /// lets the borderless child show the desktop through skin shape masks.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.; 4]
    }
}
