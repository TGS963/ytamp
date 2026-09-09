//! Offline compatibility checks using the production Winamp view and reducer.
//! cargo run --offline --example skin_compat_probe -- path/to/skins/*.wsz
//! No credentials, network requests, audio output, or account writes.
use egui::{Event, PointerButton, Pos2, Rect, Vec2};
use std::{path::Path, sync::Arc};
use ytamp::{
    core::{state::State, update::update},
    skin::{Skin, layout},
    ui::winamp::{self, WinampShell},
};

struct Probe {
    ctx: egui::Context,
    state: State,
    shell: WinampShell,
    time: f64,
    meshes: Vec<egui::ClippedPrimitive>,
}
impl Probe {
    fn new(skin: Arc<Skin>, scale: u8) -> Self {
        let mut state = State::default();
        state.winamp.scale = scale;
        let mut shell = WinampShell::default();
        shell.wear(skin);
        let mut probe = Self {
            ctx: egui::Context::default(),
            state,
            shell,
            time: 0.,
            meshes: vec![],
        };
        probe.frame(vec![]);
        probe.frame(vec![]);
        probe
    }
    fn frame(&mut self, events: Vec<Event>) {
        self.time += 0.05;
        let mut actions = Vec::new();
        let scale = self.state.winamp.scale as f32;
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(275. * scale, 700. * scale),
                )),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        winamp::show(ui, &self.state, &mut self.shell, &mut actions)
                    });
            },
        );
        self.meshes = self
            .ctx
            .tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        output.textures_delta.clear();
        for action in actions {
            update(&mut self.state, action, &mut |_| 0);
        }
    }
    fn click(&mut self, x: f32, y: f32) {
        let pos = Pos2::new(x, y) * self.state.winamp.scale as f32;
        self.frame(vec![Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            self.frame(vec![Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }]);
        }
        self.frame(vec![]);
    }
    fn masks(&self, skin: &Skin) -> Result<(), String> {
        for (mask, offset) in [
            (skin.regions.normal.as_ref(), 0.),
            (
                skin.regions.equalizer.as_ref(),
                layout::WINDOW_HEIGHT as f32,
            ),
        ] {
            let Some(mask) = mask else {
                continue;
            };
            for y in (0..mask.height).step_by(11) {
                for x in (0..mask.width).step_by(13) {
                    if mask.contains(x, y) {
                        continue;
                    }
                    let point = Pos2::new(x as f32 + 0.5, offset + y as f32 + 0.5)
                        * self.state.winamp.scale as f32;
                    if self.meshes.iter().any(|primitive| covers(primitive, point)) {
                        return Err(format!(
                            "paint escaped region mask at skin pixel {x},{y}, offset {offset}"
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}
fn covers(primitive: &egui::ClippedPrimitive, point: Pos2) -> bool {
    if !primitive.clip_rect.contains(point) {
        return false;
    }
    let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
        return false;
    };
    mesh.indices.chunks_exact(3).any(|tri| {
        let [a, b, c] = [
            mesh.vertices[tri[0] as usize],
            mesh.vertices[tri[1] as usize],
            mesh.vertices[tri[2] as usize],
        ];
        if a.color.a() == 0 && b.color.a() == 0 && c.color.a() == 0 {
            return false;
        }
        let cross =
            |a: Pos2, b: Pos2, p: Pos2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        let area = cross(a.pos, b.pos, c.pos);
        if area.abs() < 1e-5 {
            return false;
        }
        let signs = [
            cross(a.pos, b.pos, point),
            cross(b.pos, c.pos, point),
            cross(c.pos, a.pos, point),
        ];
        signs.iter().all(|s| *s >= 0.) || signs.iter().all(|s| *s <= 0.)
    })
}
fn check(skin: Arc<Skin>, scale: u8) -> Result<(), String> {
    let mut p = Probe::new(skin.clone(), scale);
    p.click(214., 63.);
    if p.state.playback.balance < 0.95 {
        return Err("main balance did not reach right".into());
    }
    p.click(178., 63.);
    if p.state.playback.balance > -0.95 {
        return Err("main balance did not reach left".into());
    }
    p.click(230., 64.);
    if !p.shell.equalizer_open {
        return Err("EQ panel did not open".into());
    }
    p.click(27., 140.);
    if !p.state.equalizer.parameters.enabled {
        return Err("EQ On control did not enable audio processing".into());
    }
    for band in 0..10 {
        p.click(85. + band as f32 * 18., 160.);
        if p.state.equalizer.parameters.bands_db[band] < 10. {
            return Err(format!("band {band} did not respond"));
        }
    }
    p.click(28., 212.);
    if p.state.equalizer.parameters.preamp_db > -10. {
        return Err("preamp did not respond".into());
    }
    p.frame(vec![Event::PointerMoved(
        Pos2::new(270., 110.) * scale as f32,
    )]);
    p.masks(&skin)?;
    p.click(252., 64.);
    if !p.shell.playlist_open {
        return Err("playlist did not open".into());
    }
    let tracks = ["current", "first", "second", "third"]
        .map(|name| ytamp::core::model::Track {
            source: Default::default(),
            id: ytamp::core::model::TrackId(name.into()),
            title: name.into(),
            artists: vec![],
            album: None,
            album_id: None,
            duration: None,
            thumbnail_url: None,
            playlist_item_id: None,
        })
        .to_vec();
    p.state.playback.queue.play_context(tracks, 0, &mut |_| 0);
    p.frame(vec![]);
    p.frame(vec![]);
    let from = Pos2::new(100., 297.) * scale as f32;
    let to = Pos2::new(100., 266.) * scale as f32;
    p.frame(vec![Event::PointerMoved(from)]);
    p.frame(vec![Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    }]);
    p.frame(vec![Event::PointerMoved(
        from + egui::vec2(0., -7.) * scale as f32,
    )]);
    p.frame(vec![Event::PointerMoved(to)]);
    p.frame(vec![Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);
    p.frame(vec![]);
    if p.state.playback.queue.peek_next().unwrap().title != "third" {
        return Err("playlist drag did not reorder".into());
    }
    p.click(100., 270.);
    p.frame(vec![Event::Key {
        key: egui::Key::Delete,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]);
    if p.state.playback.queue.upcoming().count() != 2
        || p.state.playback.queue.peek_next().unwrap().title != "first"
    {
        return Err("playlist Delete did not remove selection".into());
    }
    p.click(259., 123.);
    if !p.shell.equalizer_shade {
        return Err("EQ shade did not open".into());
    }
    p.click(206., 123.);
    if p.state.playback.balance < 0.95 {
        return Err("shaded balance did not reach right".into());
    }
    p.click(165., 123.);
    if p.state.playback.balance > -0.95 {
        return Err("shaded balance did not reach left".into());
    }
    p.click(268., 123.);
    if p.shell.equalizer_open {
        return Err("EQ shade close did not respond".into());
    }
    if !p.state.equalizer.parameters.enabled {
        return Err("closing EQ incorrectly disabled processing".into());
    }
    Ok(())
}
fn main() {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("Usage: cargo run --offline --example skin_compat_probe -- path/to/skins/*.wsz");
        std::process::exit(2);
    }
    let mut failed = 0;
    for path in paths {
        match Skin::load(Path::new(&path)) {
            Err(error) => {
                eprintln!("FAIL {path}: {error}");
                failed += 1;
            }
            Ok(skin) => {
                let skin = Arc::new(skin);
                for scale in [1, 2, 4] {
                    match check(skin.clone(), scale) {
                        Ok(()) => println!(
                            "PASS {} at {scale}x: EQ, ten bands, preamp, balance, masks, playlist, shade/close",
                            skin.name
                        ),
                        Err(error) => {
                            eprintln!("FAIL {} at {scale}x: {error}", skin.name);
                            failed += 1;
                        }
                    }
                }
            }
        }
    }
    if failed > 0 {
        std::process::exit(1);
    }
}
