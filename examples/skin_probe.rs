//! Loads a Winamp skin and reports what it holds. Usage:
//! `cargo run --example skin_probe [path/to/skin.wsz]`. With no path, it
//! loads the built-in skin of ytamp.
//!
//! It also composes the main window at 2x from `MAIN_BACKGROUND`, the
//! title bar, and the transport buttons, and writes it to `skin_probe.png`
//! in the current directory. This is the same registration check that
//! `default_skin.rs --preview` of fastpotify runs, in a minimal form.

use std::path::Path;
use std::sync::Arc;

use ytamp::skin::Skin;
use ytamp::skin::layout::{self, Area};
use ytamp::skin::sprites::{self, Sheet, Sprite};

fn main() {
    let path = std::env::args().nth(1);
    let skin = load_skin(path.as_deref());

    print_sheet_report(&skin);
    print_playlist_colors(&skin);
    print_vis_colors(&skin);
    print_region_report(&skin);

    let image = compose_main_window(&skin);
    let scaled = scale_nearest(&image, 2);
    let out_path = "skin_probe.png";
    scaled.save(out_path).expect("the preview can be written");
    println!("wrote the preview to {out_path}");
}

/// Loads a skin from a `.wsz` path, or the built-in skin when none is given.
fn load_skin(path: Option<&str>) -> Arc<Skin> {
    match path {
        Some(path) => {
            let skin =
                Skin::load(Path::new(path)).unwrap_or_else(|error| panic!("{path}: {error}"));
            Arc::new(skin)
        }
        None => Skin::builtin(),
    }
}

fn print_sheet_report(skin: &Skin) {
    println!("skin: {}", skin.name);
    println!("sheets:");
    for sheet in Sheet::ALL {
        if skin.has(sheet) {
            let bitmap = skin.sheet(sheet);
            println!(
                "  {} decoded ({}x{})",
                sheet.file_stem(),
                bitmap.width,
                bitmap.height
            );
        } else {
            println!(
                "  {} fell back to a substitute or the built-in skin",
                sheet.file_stem()
            );
        }
    }
}

fn print_playlist_colors(skin: &Skin) {
    let playlist = &skin.playlist;
    println!("pledit.txt colors:");
    println!("  normal: {:?}", playlist.normal);
    println!("  current: {:?}", playlist.current);
    println!("  normal background: {:?}", playlist.normal_background);
    println!("  selected background: {:?}", playlist.selected_background);
    println!("  font: {}", playlist.font);
}

fn print_vis_colors(skin: &Skin) {
    println!("viscolor.txt colors: {}", skin.vis_colors.len());
}

fn print_region_report(skin: &Skin) {
    let regions = &skin.regions;
    println!("region masks:");
    println!("  normal: {}", regions.normal.is_some());
    println!("  shade: {}", regions.shade.is_some());
    println!("  equalizer: {}", regions.equalizer.is_some());
    println!("  equalizer shade: {}", regions.equalizer_shade.is_some());
}

/// Draws the main window at 1x: the background, the title bar, and the
/// transport row, each at its `Area` position. A registration check: a
/// skin whose sprite table is wrong shows as misaligned art here.
fn compose_main_window(skin: &Skin) -> image::RgbaImage {
    let mut image = image::RgbaImage::new(layout::WINDOW_WIDTH, layout::WINDOW_HEIGHT);
    blit(
        skin,
        &mut image,
        sprites::MAIN_BACKGROUND,
        Area::new(0, 0, 275, 116),
    );
    blit(
        skin,
        &mut image,
        sprites::TITLE_BAR_ACTIVE,
        layout::TITLE_BAR,
    );
    for (sprite, area) in transport_buttons() {
        blit(skin, &mut image, sprite, area);
    }
    image
}

/// The resting sprites of the transport row, each paired with the area it
/// sits in the main window.
fn transport_buttons() -> [(Sprite, Area); 6] {
    [
        (sprites::PREVIOUS, layout::PREVIOUS),
        (sprites::PLAY, layout::PLAY),
        (sprites::PAUSE, layout::PAUSE),
        (sprites::STOP, layout::STOP),
        (sprites::NEXT, layout::NEXT),
        (sprites::EJECT, layout::EJECT),
    ]
}

/// Copies the pixels of a sprite onto `image` at `(area.x, area.y)`. A
/// sprite the skin has no room for draws nothing, the same as Winamp.
fn blit(skin: &Skin, image: &mut image::RgbaImage, sprite: Sprite, area: Area) {
    let Some((bitmap, sprite)) = skin.sprite(sprite) else {
        return;
    };
    for dy in 0..sprite.height {
        for dx in 0..sprite.width {
            let Some(pixel) = bitmap.pixel(sprite.x + dx, sprite.y + dy) else {
                continue;
            };
            let (x, y) = (area.x + dx, area.y + dy);
            if x < image.width() && y < image.height() {
                image.put_pixel(x, y, image::Rgba(pixel));
            }
        }
    }
}

/// Scales an image up by an integer factor with nearest-neighbour
/// sampling, which keeps a pixel-art skin crisp.
fn scale_nearest(image: &image::RgbaImage, factor: u32) -> image::RgbaImage {
    image::imageops::resize(
        image,
        image.width() * factor,
        image.height() * factor,
        image::imageops::FilterType::Nearest,
    )
}
