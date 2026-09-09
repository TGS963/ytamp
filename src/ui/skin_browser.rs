//! Native skin gallery. Archive decoding runs off the UI thread; textures stay here.
use crate::{
    core::{action::Action, state::State},
    skin::{Bitmap, Skin, layout, sprites},
};
use std::sync::mpsc::{Receiver, channel};

struct Entry {
    name: Option<String>,
    preview: Result<egui::TextureHandle, String>,
}
type Preview = (Option<String>, Result<Bitmap, String>);
#[derive(Default)]
pub struct SkinBrowser {
    entries: Vec<Entry>,
    pending: Option<Receiver<Preview>>,
    selected: usize,
    filter: String,
    initialized: bool,
}
impl SkinBrowser {
    pub fn invalidate(&mut self) {
        self.initialized = false;
        self.pending = None;
    }
    fn refresh(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() {
            return;
        }
        self.initialized = true;
        self.entries.clear();
        self.selected = 0;
        let (send, receive) = channel();
        self.pending = Some(receive);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let names =
                std::iter::once(None).chain(crate::skins_dir::list_skins().into_iter().map(Some));
            for name in names {
                let skin = match &name {
                    None => Ok(Skin::builtin()),
                    Some(name) => crate::skins_dir::skin_path(name)
                        .ok_or_else(|| "This skin is no longer in the skins folder.".to_owned())
                        .and_then(|path| {
                            Skin::load(&path)
                                .map(std::sync::Arc::new)
                                .map_err(|e| e.to_string())
                        }),
                };
                if send.send((name, skin.map(|skin| preview(&skin)))).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
            drop(send);
            ctx.request_repaint();
        });
    }
    pub fn view(&mut self, ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
        if !self.initialized {
            self.refresh(ui.ctx());
        }
        if let Some(receiver) = &self.pending {
            loop {
                match receiver.try_recv() {
                    Ok((name, bitmap)) => {
                        let texture = bitmap.map(|bitmap| {
                            ui.ctx().load_texture(
                                format!("skin-preview-{name:?}"),
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [bitmap.width as usize, bitmap.height as usize],
                                    &bitmap.rgba,
                                ),
                                egui::TextureOptions::NEAREST,
                            )
                        });
                        self.entries.push(Entry {
                            name,
                            preview: texture,
                        });
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.pending = None;
                        break;
                    }
                }
            }
        }
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Winamp skins");
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.pending.is_none(), egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.refresh(ui.ctx());
                }
                if ui.button("Open skins folder").clicked() {
                    crate::skins_dir::open_folder();
                }
                if self.pending.is_some() {
                    ui.spinner();
                }
            });
            ui.label("Drop a classic .wsz or .zip skin here to install it.");
            for notice in state.notices.iter().rev().take(1) {
                ui.weak(notice);
            }
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter skins"));
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("skin-gallery")
                .show(ui, |ui| {
                    for (index, entry) in self.entries.iter().enumerate() {
                        let name = entry.name.as_deref().unwrap_or("Built-in skin");
                        if !name.to_lowercase().contains(&self.filter.to_lowercase()) {
                            continue;
                        }
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                if let Ok(texture) = &entry.preview
                                    && ui
                                        .add(
                                            egui::Image::new((
                                                texture.id(),
                                                egui::vec2(137.5, 58.0),
                                            ))
                                            .sense(egui::Sense::click()),
                                        )
                                        .clicked()
                                {
                                    self.selected = index;
                                }
                                ui.vertical(|ui| {
                                    if ui.selectable_label(self.selected == index, name).clicked() {
                                        self.selected = index;
                                    }
                                    if entry.name == state.winamp.skin {
                                        ui.weak("In use");
                                    }
                                    if let Err(error) = &entry.preview {
                                        ui.label(error);
                                    }
                                });
                            });
                            if self.selected == index
                                && let Ok(texture) = &entry.preview
                            {
                                let width = ui.available_width().min(550.0);
                                ui.add(egui::Image::new((
                                    texture.id(),
                                    egui::vec2(width, width * 116.0 / 275.0),
                                )));
                                if ui.button("Use this skin").clicked() {
                                    out.push(Action::SkinChosen(entry.name.clone()));
                                }
                            }
                            ui.separator();
                        });
                    }
                });
        });
        let dropped = super::winamp::dropped_skins(ui.ctx());
        if !dropped.is_empty() {
            self.initialized = false;
        }
        out.extend(dropped);
    }
}
/// Assemble the idle player's artwork from the same sprite/layout tables as playback.
fn preview(skin: &Skin) -> Bitmap {
    let mut bitmap = Bitmap {
        width: 275,
        height: 116,
        rgba: vec![0; 275 * 116 * 4],
    };
    let parts = [
        (sprites::MAIN_BACKGROUND, layout::Area::new(0, 0, 275, 116)),
        (sprites::TITLE_BAR_ACTIVE, layout::TITLE_BAR),
        (sprites::OPTIONS_BUTTON, layout::OPTIONS_BUTTON),
        (sprites::MINIMIZE_BUTTON, layout::MINIMIZE_BUTTON),
        (sprites::SHADE_BUTTON, layout::SHADE_BUTTON),
        (sprites::CLOSE_BUTTON, layout::CLOSE_BUTTON),
        (sprites::CLUTTER_BAR, layout::CLUTTER_BAR),
        (sprites::PREVIOUS, layout::PREVIOUS),
        (sprites::PLAY, layout::PLAY),
        (sprites::PAUSE, layout::PAUSE),
        (sprites::STOP, layout::STOP),
        (sprites::NEXT, layout::NEXT),
        (sprites::EJECT, layout::EJECT),
        (sprites::SHUFFLE_OFF, layout::SHUFFLE),
        (sprites::REPEAT_OFF, layout::REPEAT),
        (sprites::EQ_OFF, layout::EQ_BUTTON),
        (sprites::PLAYLIST_OFF, layout::PLAYLIST_BUTTON),
    ];
    for (sprite, area) in parts {
        let Some((sheet, sprite)) = skin.sprite(sprite) else {
            continue;
        };
        for y in 0..sprite.height.min(116 - area.y) {
            for x in 0..sprite.width.min(275 - area.x) {
                let from = (((sprite.y + y) * sheet.width + sprite.x + x) * 4) as usize;
                let to = (((area.y + y) * 275 + area.x + x) * 4) as usize;
                bitmap.rgba[to..to + 4].copy_from_slice(&sheet.rgba[from..from + 4]);
            }
        }
    }
    if let Some(mask) = &skin.regions.normal {
        for y in 0..116 {
            for x in 0..275 {
                if !mask.contains(x, y) {
                    bitmap.rgba[((y * 275 + x) * 4 + 3) as usize] = 0;
                }
            }
        }
    }
    bitmap
}
