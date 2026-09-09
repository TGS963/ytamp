//! Native layout preview from local response fixtures. No auth, playback, or account writes.
//! cargo run --example discovery_preview -- /path/to/home.json /path/to/continuation.json
use ytamp::{
    core::{
        state::{AuthState, Loadable, Page, State},
        update::update,
    },
    theme::DefaultTheme,
};
struct Preview(State);
impl eframe::App for Preview {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        for action in ytamp::ui::view(ui, &self.0, &DefaultTheme) {
            let _ = update(&mut self.0, action, &mut |_| 0);
        }
    }
}
fn main() -> eframe::Result {
    let mut state = State {
        auth: AuthState::SignedIn,
        page: Page::Home,
        ..Default::default()
    };
    state.library.playlists = Loadable::Loaded(vec![]);
    state.library.liked = Loadable::Loaded(vec![]);
    let recovery = std::env::args().any(|a| a == "--recovery");
    if recovery {
        state.auth = AuthState::ConnectionFailed;
        state.sign_in.saved_account = true;
    }
    let check = std::env::args().any(|a| a == "--check");
    for file in std::env::args()
        .skip(1)
        .filter(|a| a != "--check" && a != "--recovery")
    {
        let data = std::fs::read(file).expect("read fixture");
        let json: serde_json::Value = serde_json::from_slice(&data).expect("parse fixture");
        let page = if let Some(id) = json
            .pointer("/contents/singleColumnWatchNextResults/playlist/playlist/playlistId")
            .and_then(serde_json::Value::as_str)
        {
            ytamp::api::discovery::parse_radio(&json, id)
        } else {
            ytamp::api::discovery::parse(&json)
        }
        .expect("parse feed");
        println!(
            "Fixture: {} shelves, {} tracks",
            page.shelves.len(),
            page.tracks.len()
        );
        state.discovery.home.page.shelves.extend(page.shelves);
    }
    if check {
        return Ok(());
    }
    state.discovery.home.loaded = true;
    eframe::run_native(
        "ytamp discovery preview",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1400., 950.])
                .with_min_inner_size([700., 480.]),
            ..Default::default()
        },
        Box::new(|creation| {
            ytamp::fonts::install(&creation.egui_ctx);
            egui_extras::install_image_loaders(&creation.egui_ctx);
            Ok(Box::new(Preview(state)))
        }),
    )
}
