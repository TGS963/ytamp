//! Shared artwork and desktop identity for every app window.
use std::sync::{Arc, OnceLock};

const APP_PNG: &[u8] = include_bytes!("../assets/branding/icon.png");

pub fn viewport() -> egui::ViewportBuilder {
    static ICON: OnceLock<Arc<egui::IconData>> = OnceLock::new();
    let icon = ICON.get_or_init(|| {
        let pixels = image::load_from_memory(APP_PNG)
            .expect("bundled app icon is a valid PNG")
            .into_rgba8();
        Arc::new(egui::IconData {
            width: pixels.width(),
            height: pixels.height(),
            rgba: pixels.into_raw(),
        })
    });
    egui::ViewportBuilder::default()
        .with_icon(icon.clone())
        .with_app_id("io.github.TGS963.ytamp")
}

#[cfg(target_os = "macos")]
fn native_image(bytes: &[u8]) -> objc2::rc::Retained<objc2_app_kit::NSImage> {
    use objc2::AnyThread;
    use objc2_app_kit::NSImage;
    use objc2_foundation::NSData;
    NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))
        .expect("bundled native icon is a valid PNG")
}

#[cfg(target_os = "macos")]
pub fn install_dock_icon() {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;
    let mtm = MainThreadMarker::new().expect("Dock icon runs on the app thread");
    // SAFETY: the bundled image is valid and AppKit retains it for the app.
    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&native_image(APP_PNG)));
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install_dock_icon() {}

#[cfg(target_os = "macos")]
pub(crate) fn status_image() -> objc2::rc::Retained<objc2_app_kit::NSImage> {
    let image = native_image(include_bytes!("../assets/branding/status-template.png"));
    image.setSize(objc2_foundation::NSSize::new(18., 18.));
    image.setTemplate(true);
    image
}
