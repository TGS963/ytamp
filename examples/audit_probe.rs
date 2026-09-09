//! Offline audit probes. Report observations without network or account writes.
//! Run: cargo run --offline --example audit_probe
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
static FAILURES: AtomicUsize = AtomicUsize::new(0);
use ytamp::core::{
    action::Action,
    effect::{ApiRequest, Effect},
    model::*,
    state::*,
    update::update,
};
use ytamp::theme::DefaultTheme;

fn track(id: &str) -> Track {
    Track {
        id: TrackId(id.into()),
        title: format!("Title {id}"),
        artists: vec![ArtistRef::named("Artist link")],
        album: Some("Album link".into()),
        album_id: Some(AlbumId("album".into())),
        duration: Some(Duration::from_secs(200)),
        thumbnail_url: None,
        playlist_item_id: Some(format!("item-{id}")),
    }
}
fn apply(s: &mut State, a: Action) -> Vec<Effect> {
    update(s, a, &mut |_| 0)
}
fn playlist(id: &str) -> Playlist {
    Playlist {
        id: PlaylistId(id.into()),
        title: id.into(),
        track_count: Some(1),
        thumbnail_url: None,
    }
}
fn state() -> State {
    let mut s = State {
        auth: AuthState::SignedIn,
        page: Page::Playlist(PlaylistId("p".into())),
        ..State::default()
    };
    s.library.open_playlist = Loadable::Loaded(vec![track("a"), track("b")]);
    s.library.liked = Loadable::Loaded(vec![]);
    s.library.playlists = Loadable::Loaded(vec![playlist("p")]);
    s
}
fn report(name: &str, ok: bool) {
    if !ok {
        FAILURES.fetch_add(1, Ordering::Relaxed);
    }
    println!("{} {name}", if ok { "PASS" } else { "FAIL" });
}
fn text_shapes(shape: &egui::Shape, texts: &mut Vec<(String, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => texts.push((
            t.galley.text().into(),
            egui::Rect::from_min_size(t.pos, t.galley.size()),
        )),
        egui::Shape::Vec(v) => {
            for s in v {
                text_shapes(s, texts);
            }
        }
        _ => {}
    }
}
struct UiProbe {
    ctx: egui::Context,
    time: f64,
    text: Vec<(String, egui::Rect)>,
}
impl UiProbe {
    fn new(s: &State) -> Self {
        let mut p = Self {
            ctx: egui::Context::default(),
            time: 0.,
            text: vec![],
        };
        for _ in 0..3 {
            p.frame(s, vec![]);
        }
        p
    }
    fn frame(&mut self, s: &State, events: Vec<egui::Event>) -> Vec<Action> {
        self.time += 0.016;
        let mut actions = vec![];
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200., 720.),
                )),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| actions = ytamp::ui::view(ui, s, &DefaultTheme),
        );
        output.textures_delta.clear();
        self.text.clear();
        for shape in output.shapes {
            text_shapes(&shape.shape, &mut self.text);
        }
        actions
    }
    fn pos(&self, label: &str) -> egui::Pos2 {
        self.text
            .iter()
            .find(|(s, _)| s == label)
            .unwrap_or_else(|| panic!("missing {label}: {:?}", self.text))
            .1
            .center()
    }
    fn click(&mut self, s: &State, pos: egui::Pos2, button: egui::PointerButton) -> Vec<Action> {
        self.frame(s, vec![egui::Event::PointerMoved(pos)]);
        self.frame(
            s,
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers: Default::default(),
            }],
        );
        let a = self.frame(
            s,
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        for _ in 0..3 {
            self.frame(s, vec![]);
        }
        a
    }
}
fn run_checks() -> usize {
    FAILURES.store(0, Ordering::Relaxed);
    for label in ["Title a", "Artist link", "Album link", "+"] {
        let s = state();
        let mut p = UiProbe::new(&s);
        let pos = p.pos(label);
        p.click(&s, pos, egui::PointerButton::Secondary);
        let open = egui::Popup::is_any_open(&p.ctx);
        report(&format!("right click {label:?} opens row menu"), open);
        if open {
            let pos = p.pos("Add to queue");
            let actions = p.click(&s, pos, egui::PointerButton::Primary);
            report(
                "menu queues the intended track",
                actions
                    .iter()
                    .any(|a| matches!(a,Action::TrackQueued(t) if t.id==TrackId("a".into()))),
            );
        }
    }
    let mut s = state();
    s.library.playlists = Loadable::Loaded(
        (0..80)
            .map(|i| playlist(&format!("Playlist {i}")))
            .collect(),
    );
    let mut p = UiProbe::new(&s);
    let pos = p.pos("Title a");
    p.click(&s, pos, egui::PointerButton::Secondary);
    let pos = p.pos("Add to playlist");
    p.frame(&s, vec![egui::Event::PointerMoved(pos)]);
    for _ in 0..40 {
        p.frame(&s, vec![]);
    }
    // The foreground submenu is painted after the sidebar shortcut with the same name.
    let pos = p
        .text
        .iter()
        .rev()
        .find(|(label, _)| label == "Playlist 0")
        .expect("playlist submenu")
        .1
        .center();
    p.frame(&s, vec![egui::Event::PointerMoved(pos)]);
    for _ in 0..10 {
        p.frame(
            &s,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0.0, -2000.0),
                modifiers: Default::default(),
            }],
        );
        for _ in 0..20 {
            p.frame(&s, vec![]);
        }
    }
    report(
        "large playlist submenu allows scrolling to New playlist",
        p.text
            .iter()
            .any(|(text, rect)| text == "New playlist..." && rect.center().y < 720.0),
    );
    report(
        "large playlist submenu reaches the final playlist",
        p.text
            .iter()
            .any(|(text, rect)| text == "Playlist 79" && rect.center().y < 720.0),
    );
    let mut s = state();
    s.page = Page::Search;
    apply(&mut s, Action::SearchInputChanged("old".into()));
    apply(&mut s, Action::SearchSubmitted);
    apply(&mut s, Action::SearchInputChanged("new".into()));
    apply(&mut s, Action::SearchSubmitted);
    apply(
        &mut s,
        Action::SearchLoaded(
            2,
            Ok(SearchResults {
                songs: vec![track("new")],
                ..Default::default()
            }),
        ),
    );
    apply(
        &mut s,
        Action::SearchLoaded(
            1,
            Ok(SearchResults {
                songs: vec![track("old")],
                ..Default::default()
            }),
        ),
    );
    report(
        "late search response cannot replace newer results",
        s.search.results.loaded().unwrap().songs[0].id == TrackId("new".into()),
    );
    let mut s = state();
    apply(&mut s, Action::SignOutRequested);
    apply(
        &mut s,
        Action::ForSession {
            generation: 0,
            action: Box::new(Action::LikedPageLoaded {
                tracks: vec![track("old-account")],
                finished: true,
            }),
        },
    );
    report(
        "signed-out state rejects old account library response",
        s.library.liked.loaded().is_none(),
    );
    let mut s = state();
    apply(&mut s, Action::SignOutRequested);
    apply(
        &mut s,
        Action::ForSession {
            generation: 0,
            action: Box::new(Action::AuthVerified(Ok(()))),
        },
    );
    report(
        "late auth response cannot undo sign out",
        s.auth == AuthState::SignedOut,
    );
    let mut s = state();
    apply(&mut s, Action::CreatePlaylistDialogOpened(Some(track("a"))));
    apply(&mut s, Action::PlaylistCreateRequested("first".into()));
    apply(&mut s, Action::CreatePlaylistDialogOpened(Some(track("b"))));
    apply(&mut s, Action::PlaylistCreateRequested("second".into()));
    let effects = apply(&mut s, Action::PlaylistCreated(1, Ok(playlist("first"))));
    report("overlapping playlist creation preserves the first track",effects.iter().any(|e|matches!(e,Effect::Api(ApiRequest::AddToPlaylist{playlist:p,track:t}) if p.0=="first"&&t.id.0=="a")));
    let effects = apply(&mut s, Action::PlaylistCreated(2, Ok(playlist("second"))));
    report("overlapping playlist creation preserves the second track",effects.iter().any(|e|matches!(e,Effect::Api(ApiRequest::AddToPlaylist{playlist:p,track:t}) if p.0=="second"&&t.id.0=="b")));
    let mut s = state();
    s.library.liked = Loadable::Failed("offline".into());
    s.library.playlists = Loadable::Failed("offline".into());
    apply(&mut s, Action::NavigatedTo(Page::Search));
    let effects = apply(&mut s, Action::NavigatedTo(Page::Library));
    report(
        "reopening failed library retries requests",
        effects.iter().any(|e| {
            matches!(
                e,
                Effect::Api(ApiRequest::FetchLiked | ApiRequest::FetchPlaylists)
            )
        }),
    );
    let mut s = state();
    apply(
        &mut s,
        Action::ContextPlayed {
            tracks: vec![track("playing"), track("upcoming")],
            start: 0,
        },
    );
    s.queue_open = true;
    let mut p = UiProbe::new(&s);
    let pos = p.pos("Title upcoming");
    let first = p.click(&s, pos, egui::PointerButton::Primary);
    report(
        "single queue click selects without playing",
        !first.iter().any(|a| matches!(a, Action::QueueJumped(_))),
    );
    let a = p.click(&s, pos, egui::PointerButton::Primary);
    report(
        "double-clicking a queue row jumps to its track",
        a.iter().any(|a| matches!(a, Action::QueueJumped(_))),
    );
    let mut s = state();
    apply(
        &mut s,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    let effects = apply(&mut s, Action::PlayToggled);
    report(
        "pause while loading changes playback intent",
        !effects.is_empty() && s.playback.status != PlayStatus::Loading,
    );
    let failures = FAILURES.load(Ordering::Relaxed);
    println!("{failures} failed audit checks");
    failures
}

fn main() {
    if run_checks() > 0 {
        std::process::exit(1);
    }
}

#[test]
fn audited_interactions_and_async_races() {
    assert_eq!(
        run_checks(),
        0,
        "audit regression; see the individual checks above"
    );
}
