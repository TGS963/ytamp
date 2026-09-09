//! Bundled fallback shared by the normal UI and Winamp's pixel rasterizer.
pub static JAPANESE: &[u8] = include_bytes!("../assets/fonts/NotoSansJP.ttf");

pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Noto Sans JP".into(),
        egui::FontData::from_static(JAPANESE).into(),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("Noto Sans JP".into());
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use skrifa::MetadataProvider;
    #[test]
    fn bundled_fallback_covers_japanese_titles() {
        let face = skrifa::FontRef::new(super::JAPANESE).unwrap();
        for c in "初音ミク東京アメジスト".chars() {
            assert!(face.charmap().map(c).is_some(), "missing {c}");
        }
    }
}
