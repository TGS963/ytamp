//! Native macOS menu-bar transport. Other platforms keep OS media controls.
use crate::core::state::State;
#[derive(Clone, Copy, Debug)]
pub enum MenuAction {
    PlayPause,
    Previous,
    Next,
    Visibility,
    Lyrics,
    Quit,
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use objc2::rc::Retained;
    use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
    use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString};
    use std::sync::mpsc::{Receiver, Sender, channel};
    struct TargetIvars {
        sender: Sender<MenuAction>,
        ctx: egui::Context,
    }
    define_class!(
        // SAFETY: NSObject has no subclassing requirements. Target has no Drop.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[name = "YtampMenuTarget"]
        #[ivars = TargetIvars]
        struct Target;
        // SAFETY: NSObjectProtocol has no additional requirements.
        unsafe impl NSObjectProtocol for Target {}
        impl Target {
            // SAFETY: AppKit target/action method receives its NSMenuItem sender.
            #[unsafe(method(performMenuAction:))]
            fn perform(&self, item: &NSMenuItem) {
                let action = match item.tag() { 1 => MenuAction::PlayPause, 2 => MenuAction::Previous, 3 => MenuAction::Next, 4 => MenuAction::Visibility, 5 => MenuAction::Lyrics, 6 => MenuAction::Quit, _ => return };
                let _ = self.ivars().sender.send(action);
                self.ivars().ctx.request_repaint();
            }
        }
    );
    pub struct MenuBar {
        status: Retained<NSStatusItem>,
        title: Retained<NSMenuItem>,
        play: Retained<NSMenuItem>,
        previous: Retained<NSMenuItem>,
        next: Retained<NSMenuItem>,
        visibility: Retained<NSMenuItem>,
        receiver: Receiver<MenuAction>,
        _target: Retained<Target>,
    }
    impl MenuBar {
        pub fn new(ctx: egui::Context) -> Self {
            let mtm = MainThreadMarker::new().expect("menu bar runs on the app thread");
            let (sender, receiver) = channel();
            let allocated = Target::alloc(mtm).set_ivars(TargetIvars { sender, ctx });
            // SAFETY: NSObject init initializes our freshly allocated Target.
            let target: Retained<Target> = unsafe { msg_send![super(allocated), init] };
            let menu = NSMenu::new(mtm);
            menu.setAutoenablesItems(false);
            let item = |text: &str, tag: isize| {
                let item = NSMenuItem::new(mtm);
                item.setTitle(&NSString::from_str(text));
                item.setTag(tag);
                if tag != 0 {
                    // SAFETY: Target is retained for the full menu lifetime and implements
                    // this selector with AppKit's required sender argument and void return.
                    unsafe {
                        item.setTarget(Some(&target));
                        item.setAction(Some(sel!(performMenuAction:)));
                    }
                } else {
                    item.setEnabled(false);
                }
                menu.addItem(&item);
                item
            };
            let title = item("ytamp — no track", 0);
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            let play = item("Play", 1);
            let previous = item("Previous", 2);
            let next = item("Next", 3);
            item("Lyrics", 5);
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            let visibility = item("Hide ytamp", 4);
            item("Quit ytamp", 6);
            let status = NSStatusBar::systemStatusBar().statusItemWithLength(-1.);
            status.setMenu(Some(&menu));
            if let Some(button) = status.button(mtm) {
                button.setImage(Some(&crate::branding::status_image()));
                button.setToolTip(Some(&NSString::from_str("ytamp")));
            }
            Self {
                status,
                title,
                play,
                previous,
                next,
                visibility,
                receiver,
                _target: target,
            }
        }
        pub fn drain(&self) -> Vec<MenuAction> {
            self.receiver.try_iter().collect()
        }
        pub fn sync(&self, state: &State) {
            let mtm = MainThreadMarker::new().expect("app thread");
            let current = state.playback.queue.current();
            let title = current
                .map(|t| format!("{} — {}", t.title, t.artist_names()))
                .unwrap_or_else(|| "ytamp — no track".into());
            self.title.setTitle(&NSString::from_str(
                &title.chars().take(90).collect::<String>(),
            ));
            if let Some(button) = self.status.button(mtm) {
                button.setToolTip(Some(&NSString::from_str(&title)));
            }
            self.play.setEnabled(current.is_some());
            self.previous.setEnabled(current.is_some());
            self.next.setEnabled(current.is_some());
            let playing = matches!(
                state.playback.status,
                crate::core::state::PlayStatus::Playing | crate::core::state::PlayStatus::Loading
            );
            self.play
                .setTitle(&NSString::from_str(if playing { "Pause" } else { "Play" }));
            self.visibility.setTitle(&NSString::from_str(
                if NSApplication::sharedApplication(mtm).isHidden() {
                    "Show ytamp"
                } else {
                    "Hide ytamp"
                },
            ));
        }
        pub fn show(&self) {
            let mtm = MainThreadMarker::new().expect("app thread");
            let app = NSApplication::sharedApplication(mtm);
            app.unhide(None);
            app.activate();
        }
        /// Returns true when showing; the shell then focuses the active player.
        pub fn toggle_visibility(&self) -> bool {
            let mtm = MainThreadMarker::new().expect("app thread");
            let app = NSApplication::sharedApplication(mtm);
            if app.isHidden() {
                app.unhide(None);
                app.activate();
                true
            } else {
                app.hide(None);
                false
            }
        }
    }
    impl Drop for MenuBar {
        fn drop(&mut self) {
            self.status.setMenu(None);
            NSStatusBar::systemStatusBar().removeStatusItem(&self.status);
        }
    }
}
#[cfg(target_os = "macos")]
pub use native::MenuBar;
#[cfg(not(target_os = "macos"))]
pub struct MenuBar;
#[cfg(not(target_os = "macos"))]
impl MenuBar {
    pub fn new(_: egui::Context) -> Self {
        Self
    }
    pub fn drain(&self) -> Vec<MenuAction> {
        vec![]
    }
    pub fn sync(&self, _: &State) {}
    pub fn show(&self) {}
    pub fn toggle_visibility(&self) -> bool {
        false
    }
}
