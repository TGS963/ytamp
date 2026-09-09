//! Shared default-interface controls. Hover and focus never change geometry.

mod buttons;
mod focus;
mod headings;
mod sliders;

pub use buttons::{play_button, primary, quiet, secondary};
pub use focus::focus_outline;
pub use headings::{heading, page_title, section};
pub use sliders::{signed_slider, slider};
pub const CONTROL_HEIGHT: f32 = 36.;
pub const SECTION_GAP: f32 = 12.;

#[cfg(test)]
mod tests;
