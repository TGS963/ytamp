use super::*;
#[cfg(test)]
mod layout_tests {
    use super::*;
    use crate::core::discovery::{Entry, Shelf, Target};
    #[test]
    fn offscreen_artwork_shelf_keeps_its_height() {
        let state = State::default();
        let context = crate::ui::row_context(&state);
        let shelf = Shelf {
            title: "Mixes".into(),
            entries: (0..8)
                .map(|i| Entry {
                    title: format!("Mix {i}"),
                    subtitle: String::new(),
                    artwork: None,
                    target: Target::Browse {
                        id: i.to_string(),
                        params: None,
                    },
                    track: None,
                })
                .collect(),
        };
        let ctx = egui::Context::default();
        let mut bottom = 0.;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(700., 300.),
                )),
                ..Default::default()
            },
            |ui| {
                ui.add_space(250.);
                shelf_view(
                    ui,
                    &shelf,
                    &crate::theme::DefaultTheme,
                    &context,
                    &mut vec![],
                );
                bottom = ui.cursor().top();
            },
        );
        output.textures_delta.clear();
        assert!(bottom > 550., "offscreen shelf collapsed to {bottom}");
        assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.rect.width() > 100. && r.rect.height() > 0. && r.rect.height() < 12.)), "artwork shelf paints an unnecessary horizontal scrollbar");
    }
}
